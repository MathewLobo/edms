(() => {
  "use strict";

  // ============================================================
  // CONFIG
  // ============================================================

  const DATA_URL = "../data/collections.json";

  const PAGE_SIZE = 10;

  // ============================================================
  // STATE
  // ============================================================

  const state = {
    collection: null,

    endpoints: [],

    filtered: [],

    page: 1,

    search: "",

    method: "all",

    tags: new Set(),

    segments: new Set(),

    selected: new Set(),

    selectedEndpoint: null,
  };

  // ============================================================
  // INIT
  // ============================================================

  document.addEventListener("DOMContentLoaded", init);

  async function init() {
    wireNavigation();

    wireToolbar();

    wireTable();

    wireWindows();

    wireSidebar();

    await loadCollection();

    if (typeof TagManager !== "undefined") {
      TagManager.init({
        view: "dataview",
        getItems: () =>
          state.selected.size > 0
            ? state.endpoints.filter((ep) =>
                state.selected.has(getEndpointId(ep)),
              )
            : state.endpoints,
        getTags: (endpoint) =>
          Array.isArray(endpoint.tags) ? endpoint.tags : [],
        setTags: (endpoint, tags) => {
          endpoint.tags = tags;
        },
        addTag: async (endpoint, tag) => {
          if (
            !window.EdmsAPI ||
            typeof window.EdmsAPI.addEndpointTag !== "function"
          )
            return;
          await window.EdmsAPI.addEndpointTag(getEndpointId(endpoint), tag);
          if (!Array.isArray(endpoint.tags)) endpoint.tags = [];
          if (!endpoint.tags.includes(tag)) endpoint.tags.push(tag);
        },
        removeTag: async (endpoint, tag) => {
          if (
            !window.EdmsAPI ||
            typeof window.EdmsAPI.removeEndpointTag !== "function"
          )
            return;
          await window.EdmsAPI.removeEndpointTag(getEndpointId(endpoint), tag);
          endpoint.tags = Array.isArray(endpoint.tags)
            ? endpoint.tags.filter((item) => item !== tag)
            : [];
        },
        onChange: () => {
          render();
        },
      });
    }
  }

  // ============================================================
  // LOAD COLLECTION
  // ============================================================

  async function loadCollection() {
    const params = new URLSearchParams(window.location.search);

    const folderId = params.get("folder");

    if (!folderId) {
      renderError("No collection was selected.");

      return;
    }

    try {
      const api = window.EdmsAPI;
      if (!api) {
        throw new Error("API not loaded");
      }

      const collectionResponse = await api.getCollection(folderId);
      if (!collectionResponse || collectionResponse.ok === false) {
        renderError("The requested collection could not be found.");
        return;
      }
      const colData = collectionResponse.data ?? collectionResponse;
      state.collection = colData.collection ?? colData;

      const endpointsResponse = await api.listCollectionEndpoints(folderId);
      let endpointsList = [];
      if (endpointsResponse && endpointsResponse.ok !== false) {
        const epData = endpointsResponse.data ?? endpointsResponse;
        endpointsList = Array.isArray(epData)
          ? epData
          : Array.isArray(epData?.endpoints)
            ? epData.endpoints
            : Array.isArray(epData?.items)
              ? epData.items
              : [];
      }
      state.collection.endpoints = endpointsList;

      const metadataMap = new Map();
      await new Promise((resolve) => {
        let socket;
        let resolved = false;
        const finish = () => {
          if (resolved) return;
          resolved = true;
          if (socket) socket.close();
          resolve();
        };
        const timeout = setTimeout(finish, 5000);
        try {
          socket = api.connectEndpointLoader();
          if (!socket) {
            clearTimeout(timeout);
            return finish();
          }
          socket.onmessage = (event) => {
            try {
              const message =
                typeof event.data === "string"
                  ? JSON.parse(event.data)
                  : event.data;
              function collect(val) {
                if (!val) return;
                if (Array.isArray(val)) {
                  val.forEach(collect);
                  return;
                }
                if (typeof val !== "object") return;
                const eid = val.endpoint_id ?? val.id;
                if (eid) {
                  metadataMap.set(String(eid), val);
                }
                Object.values(val).forEach(collect);
              }
              collect(message);
              if (metadataMap.size > 0) {
                clearTimeout(timeout);
                finish();
              }
            } catch (e) {
              console.error("Error parsing websocket message", e);
            }
          };
          socket.onerror = () => {
            clearTimeout(timeout);
            finish();
          };
          socket.onclose = () => {
            clearTimeout(timeout);
            finish();
          };
        } catch (e) {
          clearTimeout(timeout);
          finish();
        }
      });

      state.endpoints = state.collection.endpoints.map((endpoint) => {
        const eid = String(endpoint.endpoint_id ?? endpoint.id ?? "");
        const info = metadataMap.get(eid) || {};
        return {
          ...endpoint,
          endpoint: info.endpoint_str ?? endpoint.endpoint ?? "",
          endpoint_str: info.endpoint_str ?? endpoint.endpoint_str ?? "",
          annotation: info.annotation ?? endpoint.annotation,
          method: info.method ?? endpoint.method ?? "",
          tags: Array.isArray(info.tags) ? info.tags : (Array.isArray(endpoint.tags) ? endpoint.tags : []),
        };
      });

      if (window.EdmsAPI && typeof window.EdmsAPI.listEndpointTags === "function") {
        await Promise.all(
          state.endpoints.map(async (endpoint) => {
            try {
              const res = await window.EdmsAPI.listEndpointTags(getEndpointId(endpoint));
              const data = res?.data ?? res;
              if (data && Array.isArray(data.tags)) {
                endpoint.tags = data.tags;
              } else if (Array.isArray(data)) {
                endpoint.tags = data;
              }
            } catch (e) {
              console.error("Failed to load tags for", getEndpointId(endpoint), e);
            }
          })
        );
      }

      state.filtered = [...state.endpoints];

      renderCollectionInfo();
      updateSidebarStats();
      renderTags();
      renderSegments();
      renderStats();
      render();
    } catch (error) {
      console.error("Failed to load collection:", error);
      renderError("Unable to load collection data.");
    }
  }

  // ============================================================
  // FILTERING
  // ============================================================

  function applyFilters() {
    const term = state.search.trim().toLowerCase();

    state.filtered = state.endpoints.filter((endpoint) => {
      const path = String(endpoint.endpoint || "");

      const method = String(endpoint.method || "").toUpperCase();

      const annotation = String(endpoint.annotation || "");

      const tags = Array.isArray(endpoint.tags) ? endpoint.tags : [];

      const matchesSearch =
        !term ||
        path.toLowerCase().includes(term) ||
        method.toLowerCase().includes(term) ||
        annotation.toLowerCase().includes(term) ||
        tags.some((tag) => String(tag).toLowerCase().includes(term));

      const matchesMethod = state.method === "all" || method === state.method;

      const matchesTags = state.tags.size === 0 || [...state.tags].every((tag) => tags.includes(tag));

      const endpointSegments = getEndpointSegments(path);

      const matchesSegments = state.segments.size === 0 || [...state.segments].every((segment) => endpointSegments.includes(segment));

      return matchesSearch && matchesMethod && matchesTags && matchesSegments;
    });

    state.page = Math.min(state.page, getTotalPages());

    render();
  }

  function getTotalPages() {
    return Math.max(1, Math.ceil(state.filtered.length / PAGE_SIZE));
  }

  function getPageItems() {
    const start = (state.page - 1) * PAGE_SIZE;

    return state.filtered.slice(start, start + PAGE_SIZE);
  }

  // ============================================================
  // RENDER
  // ============================================================

  function render() {
    renderTable();

    renderPagination();

    updateSelectionUI();
  }

  // ============================================================
  // TABLE
  // ============================================================

  function renderTable() {
    const tbody = document.getElementById("endpointTableBody");

    tbody.innerHTML = "";

    const items = getPageItems();

    if (!items.length) {
      tbody.innerHTML = `

                <tr>

                    <td
                        colspan="6"
                        class="px-4 py-16
                               text-center"
                    >

                        <p
                            class="text-sm
                                   text-slate-400"
                        >
                            No endpoints found
                        </p>

                        <p
                            class="mt-1
                                   text-[11px]
                                   text-slate-600"
                        >
                            Try changing the
                            current filters.
                        </p>

                    </td>

                </tr>

            `;

      return;
    }

    items.forEach((endpoint) => {
      const row = createEndpointRow(endpoint);

      tbody.appendChild(row);
    });

    updateSelectAllState();
  }

  function createEndpointRow(endpoint) {
    const row = document.createElement("tr");

    const endpointId = getEndpointId(endpoint);

    const selected = state.selected.has(endpointId);

    const method = String(endpoint.method || "").toUpperCase();

    const tags = Array.isArray(endpoint.tags) ? endpoint.tags : [];

    const annotation = endpoint.annotation || "—";

    row.dataset.id = endpointId;

    row.className = [
      "border-b",
      "border-slate-800",
      "cursor-pointer",
      "transition-colors",
      "hover:bg-slate-800/60",

      selected ? "bg-cyan-500/[0.055]" : "",
    ]
      .filter(Boolean)
      .join(" ");

    row.innerHTML = `

            <td class="px-3 py-2">

                <input
                    type="checkbox"
                    class="endpoint-select
                           h-3.5 w-3.5
                           accent-cyan-400"
                    ${selected ? "checked" : ""}
                >

            </td>


            <td
                class="px-3 py-2
                       font-semibold
                       ${methodClass(method)}"
            >
                ${escapeHtml(method || "—")}
            </td>


            <td class="px-3 py-2">

                <div
                    class="flex items-center
                           gap-2"
                >

                    <span
                        class="font-mono
                               text-[11px]
                               text-slate-300"
                    >
                        ${renderEndpointPath(endpoint.endpoint)}
                    </span>

                </div>

            </td>


            <td class="px-3 py-2">

                <div
                    class="flex flex-wrap
                           gap-1"
                >

                    ${
                      tags.length
                        ? tags
                            .map(
                              (tag) => `

                                        <button
                                            type="button"
                                            class="endpoint-tag
                                                   rounded
                                                   bg-sky-900/40
                                                   px-1.5 py-0.5
                                                   text-[10px]
                                                   text-sky-300
                                                   hover:bg-sky-500/20"
                                            data-tag="${escapeAttr(tag)}"
                                        >
                                            ${escapeHtml(tag)}
                                        </button>

                                    `,
                            )
                            .join("")
                        : `
                                <span
                                    class="text-[10px]
                                           text-slate-600"
                                >
                                    —
                                </span>
                              `
                    }

                </div>

            </td>


            <td
                class="px-3 py-2
                       text-center"
            >

                <span
                    class="inline-flex
                           min-w-6
                           justify-center
                           rounded-md
                           bg-slate-800
                           px-1.5 py-0.5
                           text-[10px]
                           text-slate-400"
                >
                    ${getQPCount(endpoint)}
                </span>

            </td>


            <td
                class="px-3 py-2
                       text-[11px]
                       text-slate-500"
            >

                <span
                    class="line-clamp-2"
                    title="${escapeAttr(annotation)}"
                >
                    ${escapeHtml(annotation)}
                </span>

            </td>

        `;

    // --------------------------------------------------------
    // CHECKBOX
    // --------------------------------------------------------

    const checkbox = row.querySelector(".endpoint-select");

    checkbox.addEventListener("click", (event) => event.stopPropagation());

    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        state.selected.add(endpointId);
      } else {
        state.selected.delete(endpointId);
      }

      updateSelectionUI();

      updateSelectAllState();

      renderTable();
    });

    // --------------------------------------------------------
    // TAG
    // --------------------------------------------------------

    row.querySelectorAll(".endpoint-tag").forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();

        const tag = button.dataset.tag;
        state.tags.clear();
        state.tags.add(tag);

        state.page = 1;

        // Re-render sidebar to sync checkboxes
        renderTags();

        applyFilters();
      });
    });

    // --------------------------------------------------------
    // ROW CLICK
    // --------------------------------------------------------

    row.addEventListener("click", (event) => {
      if (event.target.closest("button,input")) {
        return;
      }

      selectEndpoint(endpoint);
    });

    row.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      openEndpointContextMenu(event, endpoint);
    });

    return row;
  }

  // ============================================================
  // ENDPOINT SELECTION
  // ============================================================

  async function selectEndpoint(endpoint) {
    state.selectedEndpoint = endpoint;
    openWindow("requestWindow");
    
    const eid = getEndpointId(endpoint);
    const API_BASE = "http://localhost:3000";
    let qps = [];
    try {
        const res = await fetch(`${API_BASE}/test-view/${encodeURIComponent(eid)}/qps`);
        if (res.ok) {
            const data = await res.json();
            if (data && Array.isArray(data.qps)) {
                qps = data.qps.map(qp => ({
                    id: qp.request_number,
                    name: String(qp.request_number)
                }));
            }
        }
    } catch (e) {
        console.error("Failed to fetch QPs", e);
    }

    const qpContainer = document.getElementById("qpSelectorContainer");
    if (qpContainer) {
        qpContainer.innerHTML = "";
        if (qps.length > 0) {
            qps.forEach((qp, index) => {
                const btn = document.createElement("button");
                btn.className = "px-2 py-1 text-xs rounded border border-slate-700 bg-slate-800 text-slate-300 hover:bg-slate-700 transition-colors";
                btn.textContent = `QP${qp.name}`;
                btn.onclick = () => loadQPDetails(eid, qp.id, btn);
                qpContainer.appendChild(btn);
                if (index === 0) {
                    btn.click();
                }
            });
        } else {
            qpContainer.innerHTML = `<span class="text-xs text-slate-500">No QPs</span>`;
            renderEndpointDetails(endpoint, null);
        }
    } else {
        renderEndpointDetails(endpoint, null);
    }

    renderTable();
  }

  async function loadQPDetails(endpointId, qpId, activeBtn) {
    const qpContainer = document.getElementById("qpSelectorContainer");
    if (qpContainer) {
        qpContainer.querySelectorAll("button").forEach(b => {
            b.classList.remove("border-sky-500", "text-sky-400");
            b.classList.add("border-slate-700", "text-slate-300");
        });
        activeBtn.classList.remove("border-slate-700", "text-slate-300");
        activeBtn.classList.add("border-sky-500", "text-sky-400");
    }

    const API_BASE = "http://localhost:3000";
    try {
        const [reqRes, resRes, hdrRes] = await Promise.all([
            fetch(`${API_BASE}/test-view/${encodeURIComponent(endpointId)}/request/${qpId}`).catch(() => null),
            fetch(`${API_BASE}/test-view/${encodeURIComponent(endpointId)}/response/${qpId}`).catch(() => null),
            fetch(`${API_BASE}/test-view/${encodeURIComponent(endpointId)}/headers/${qpId}`).catch(() => null)
        ]);

        const reqData = reqRes?.ok ? await reqRes.json() : null;
        const resData = resRes?.ok ? await resRes.json() : null;
        const hdrData = hdrRes?.ok ? await hdrRes.json() : null;

        const request = {
            headers: hdrData || {},
            query: reqData?.query || {},
            body: reqData?.body || null
        };
        const response = {
            status: resData?.status || null,
            body: resData?.body || resData || null
        };
        renderEndpointDetails(state.selectedEndpoint, { request, response });
    } catch (e) {
        console.error("Failed to load QP details", e);
        renderEndpointDetails(state.selectedEndpoint, null);
    }
  }

  function renderEndpointDetails(endpoint, qpData) {
    const request = qpData?.request || endpoint.request || {};

    const response = qpData?.response || endpoint.response || {};

    const reqBodyEl = document.getElementById("requestBody");
    if (reqBodyEl) {
      reqBodyEl.textContent = formatData(
        request.body ?? "Not available",
      );
    }

    document.getElementById("responseStatus").textContent = response.status
      ? String(response.status)
      : "Prototype";

    document.getElementById("responseBody").textContent = formatData(
      response.body ??
        response ?? {
          endpoint: endpoint.endpoint,

          message: "No response payload defined.",
        },
    );
  }

  // ============================================================
  // STATS
  // ============================================================

  function renderStats() {
    const endpoints = state.endpoints;

    const tags = new Set();

    let get = 0;

    let write = 0;

    endpoints.forEach((endpoint) => {
      const method = String(endpoint.method || "").toUpperCase();

      if (method === "GET") {
        get++;
      }

      if (["POST", "PUT", "PATCH", "DELETE"].includes(method)) {
        write++;
      }

      const endpointTags = Array.isArray(endpoint.tags) ? endpoint.tags : [];

      endpointTags.forEach((tag) => tags.add(tag));
    });

    document.getElementById("endpointCount").textContent = endpoints.length;

    document.getElementById("tagCount").textContent = tags.size;

    document.getElementById("getCount").textContent = get;

    document.getElementById("writeCount").textContent = write;
  }

  function renderCollectionInfo() {
    const folder = state.collection;

    document.getElementById("collectionTitle").textContent = folder.name;

    document.getElementById("collectionSubtitle").textContent = [
      capitalize(folder.purpose),
      folder.datatype || "",
    ]
      .filter(Boolean)
      .join(" · ");
  }

  // ============================================================
  // TAG FILTER
  // ============================================================

  function getEndpointSegments(endpointPath) {
    return String(endpointPath || "").split("/").filter(Boolean);
  }

  function wireSidebar() {
    const btn = document.getElementById("sidebarToggle");
    const sidebar = document.getElementById("filterSidebar");
    const content = document.getElementById("sidebarContent");
    const title = document.getElementById("sidebarTitle");
    const icon = document.getElementById("sidebarToggleIcon");

    if (!btn || !sidebar) return;

    let sidebarCollapsed = false;

    btn.addEventListener("click", () => {
      sidebarCollapsed = !sidebarCollapsed;

      if (sidebarCollapsed) {
        sidebar.classList.remove("w-48");
        sidebar.classList.add("w-10");
        content?.classList.add("hidden");
        title?.classList.add("hidden");
        if (icon) {
          icon.innerHTML = '<path d="m9 18 6-6-6-6" />';
        }
      } else {
        sidebar.classList.remove("w-10");
        sidebar.classList.add("w-48");
        content?.classList.remove("hidden");
        title?.classList.remove("hidden");
        if (icon) {
          icon.innerHTML = '<path d="m15 18-6-6 6-6" />';
        }
      }
    });
  }

  function updateSidebarStats() {
    const elEndpoints = document.getElementById("endpointStat");
    const elTags = document.getElementById("tagStat");
    const elSegments = document.getElementById("segmentStat");

    if (elEndpoints) elEndpoints.textContent = state.endpoints.length;

    const tags = new Set();
    const segments = new Set();
    state.endpoints.forEach((ep) => {
      (Array.isArray(ep.tags) ? ep.tags : []).forEach((t) => tags.add(t));
      getEndpointSegments(ep.endpoint).forEach((s) => segments.add(s));
    });

    if (elTags) elTags.textContent = tags.size;
    if (elSegments) elSegments.textContent = segments.size;
  }

  function renderTags() {
    const container = document.getElementById("tagsContainer");
    if (!container) return;

    const tagCounts = {};
    state.endpoints.forEach((ep) => {
      (Array.isArray(ep.tags) ? ep.tags : []).forEach((tag) => {
        tagCounts[tag] = (tagCounts[tag] || 0) + 1;
      });
    });

    container.innerHTML = "";

    Object.entries(tagCounts)
      .sort((a, b) => a[0].localeCompare(b[0]))
      .forEach(([tag, count]) => {
        const wrapper = document.createElement("label");
        wrapper.className = "group flex cursor-pointer items-center gap-2";
        wrapper.innerHTML = `
            <input type="checkbox" class="tag-checkbox h-3.5 w-3.5 accent-cyan-400">
            <button type="button" class="tag-filter-button min-w-0 flex-1 truncate rounded px-1.5 py-1 text-left text-[11px] text-slate-400 transition hover:bg-sky-500/10 hover:text-sky-300">
                ${escapeHtml(tag)}
                <span class="text-slate-600">(${count})</span>
            </button>
        `;

        const checkbox = wrapper.querySelector(".tag-checkbox");
        checkbox.checked = state.tags.has(tag);
        
        checkbox.addEventListener("change", () => {
          setTagFilter(tag, checkbox.checked);
        });

        wrapper.querySelector(".tag-filter-button").addEventListener("click", (e) => {
          e.preventDefault();
          const checked = !state.tags.has(tag);
          checkbox.checked = checked;
          setTagFilter(tag, checked);
        });

        container.appendChild(wrapper);
      });
  }

  function setTagFilter(tag, enabled) {
    if (enabled) {
      state.tags.add(tag);
    } else {
      state.tags.delete(tag);
    }
    state.page = 1;
    applyFilters();
  }

  function renderSegments() {
    const container = document.getElementById("segmentsContainer");
    if (!container) return;

    const segmentCounts = {};
    state.endpoints.forEach((ep) => {
      getEndpointSegments(ep.endpoint).forEach((segment) => {
        segmentCounts[segment] = (segmentCounts[segment] || 0) + 1;
      });
    });

    container.innerHTML = "";

    Object.entries(segmentCounts)
      .sort((a, b) => a[0].localeCompare(b[0]))
      .forEach(([segment, count]) => {
        const wrapper = document.createElement("label");
        wrapper.className = "group flex cursor-pointer items-center gap-2";
        wrapper.innerHTML = `
            <input type="checkbox" class="segment-checkbox h-3.5 w-3.5 accent-cyan-400">
            <button type="button" class="segment-filter-button min-w-0 flex-1 truncate rounded px-1.5 py-1 text-left text-[11px] text-slate-400 transition hover:bg-cyan-500/10 hover:text-cyan-300">
                ${escapeHtml(segment)}
                <span class="text-slate-600">(${count})</span>
            </button>
        `;

        const checkbox = wrapper.querySelector(".segment-checkbox");
        checkbox.checked = state.segments.has(segment);
        
        checkbox.addEventListener("change", () => {
          setSegmentFilter(segment, checkbox.checked);
        });

        wrapper.querySelector(".segment-filter-button").addEventListener("click", (e) => {
          e.preventDefault();
          const checked = !state.segments.has(segment);
          checkbox.checked = checked;
          setSegmentFilter(segment, checked);
        });

        container.appendChild(wrapper);
      });
  }

  function setSegmentFilter(segment, enabled) {
    if (enabled) {
      state.segments.add(segment);
    } else {
      state.segments.delete(segment);
    }
    state.page = 1;
    applyFilters();
  }

  // ============================================================
  // PAGINATION
  // ============================================================

  function renderPagination() {
    const total = state.filtered.length;

    const start = total ? (state.page - 1) * PAGE_SIZE + 1 : 0;

    const end = Math.min(state.page * PAGE_SIZE, total);

    document.getElementById("rangeStart").textContent = start;

    document.getElementById("rangeEnd").textContent = end;

    document.getElementById("totalEndpoints").textContent = total;

    const totalPages = getTotalPages();

    const container = document.getElementById("pageNumbers");

    container.innerHTML = "";

    getPageRange(state.page, totalPages).forEach((page) => {
      if (page === "...") {
        const span = document.createElement("span");

        span.className = "px-1 text-slate-600";

        span.textContent = "…";

        container.appendChild(span);

        return;
      }

      const button = document.createElement("button");

      button.type = "button";

      button.textContent = page;

      button.className =
        page === state.page
          ? `
                            min-w-7 rounded-md
                            bg-cyan-500
                            px-2 py-1
                            text-[11px] text-white
                          `
          : `
                            min-w-7 rounded-md
                            border border-slate-700
                            px-2 py-1
                            text-[11px]
                            text-slate-400
                            hover:bg-slate-800
                            hover:text-white
                          `;

      button.addEventListener("click", () => {
        state.page = page;

        render();
      });

      container.appendChild(button);
    });

    document.getElementById("prevPage").disabled = state.page <= 1;

    document.getElementById("nextPage").disabled = state.page >= totalPages;
  }

  function getPageRange(current, total) {
    if (total <= 7) {
      return Array.from(
        {
          length: total,
        },
        (_, i) => i + 1,
      );
    }

    const result = [1];

    if (current > 4) {
      result.push("...");
    }

    for (
      let page = Math.max(2, current - 1);
      page <= Math.min(total - 1, current + 1);
      page++
    ) {
      result.push(page);
    }

    if (current < total - 3) {
      result.push("...");
    }

    result.push(total);

    return result;
  }

  // ============================================================
  // SELECTION
  // ============================================================

  function updateSelectionUI() {
    const count = state.selected.size;
    const label = document.getElementById("selectionLabel");
    const clear = document.getElementById("clearEndpointSelection");

    const tagsBtn = document.getElementById("tagsManager");
    const deleteBtn = document.getElementById("deleteSelected");

    if (count) {
      label.textContent = `${count} selected`;
      label.classList.remove("hidden");
      clear.classList.remove("hidden");
      if (tagsBtn) tagsBtn.disabled = false;
      if (deleteBtn) deleteBtn.disabled = false;
    } else {
      label.classList.add("hidden");
      clear.classList.add("hidden");
      if (tagsBtn) tagsBtn.disabled = true;
      if (deleteBtn) deleteBtn.disabled = true;
    }

    updateSelectAllState();
  }

  function updateSelectAllState() {
    const selectAll = document.getElementById("selectAllEndpoints");

    const items = getPageItems();

    const selectedCount = items.filter((endpoint) =>
      state.selected.has(getEndpointId(endpoint)),
    ).length;

    selectAll.checked = items.length > 0 && selectedCount === items.length;

    selectAll.indeterminate = selectedCount > 0 && selectedCount < items.length;
  }

  // ============================================================
  // TOOLBAR
  // ============================================================

  function wireToolbar() {
    document
      .getElementById("endpointSearch")
      .addEventListener("input", (event) => {
        state.search = event.target.value;

        state.page = 1;

        applyFilters();
      });

    document
      .getElementById("methodFilter")
      .addEventListener("change", (event) => {
        state.method = event.target.value;

        state.page = 1;

        applyFilters();
      });

    // Tag filter removed as we now use sidebar

    document
      .getElementById("resetEndpointFilters")
      .addEventListener("click", resetFilters);

    document
      .getElementById("clearEndpointSelection")
      .addEventListener("click", () => {
        state.selected.clear();

        render();
      });

    document
      .getElementById("selectAllEndpoints")
      .addEventListener("change", (event) => {
        getPageItems().forEach((endpoint) => {
          const id = getEndpointId(endpoint);

          if (event.target.checked) {
            state.selected.add(id);
          } else {
            state.selected.delete(id);
          }
        });

        render();
      });

    document.getElementById("prevPage").addEventListener("click", () => {
      if (state.page > 1) {
        state.page--;

        render();
      }
    });

    document.getElementById("nextPage").addEventListener("click", () => {
      if (state.page < getTotalPages()) {
        state.page++;

        render();
      }
    });
  }

  function resetFilters() {
    state.search = "";

    state.method = "all";

    state.tags.clear();
    state.segments.clear();

    state.page = 1;

    document.getElementById("endpointSearch").value = "";

    document.getElementById("methodFilter").value = "all";

    // Uncheck sidebar inputs
    document.querySelectorAll(".tag-checkbox, .segment-checkbox").forEach(cb => cb.checked = false);

    applyFilters();
  }

  // ============================================================
  // TABLE
  // ============================================================

  function wireTable() {
    /*
     * The table itself is rendered dynamically.
     * Row-level events are attached when rows are created.
     */
  }

  // ============================================================
  // REQUEST / RESPONSE WINDOWS
  // ============================================================

  function wireWindows() {
    document.querySelectorAll("[data-window-close]").forEach((button) => {
      button.addEventListener("click", () => {
        const type = button.dataset.windowClose;

        closeWindow(type === "request" ? "requestWindow" : "responseWindow");
      });
    });
  }

  function openWindow(id) {
    document.getElementById(id)?.classList.remove("hidden");
  }

  function closeWindow(id) {
    document.getElementById(id)?.classList.add("hidden");
  }

  // ============================================================
  // NAVIGATION
  // ============================================================

  function wireNavigation() {
    document
      .getElementById("backToCollections")
      ?.addEventListener("click", () => {
        window.location.href = "./collection_view.html";
      });

    document.querySelectorAll("[data-target]").forEach((button) => {
      button.addEventListener("click", () => {
        window.location.href = button.dataset.target;
      });
    });
  }

  // ============================================================
  // ERROR
  // ============================================================

  function renderError(message) {
    document.getElementById("collectionTitle").textContent =
      "Collection Data View";

    document.getElementById("collectionSubtitle").textContent = message;

    document.getElementById("endpointTableBody").innerHTML = `

            <tr>

                <td
                    colspan="6"
                    class="px-4 py-20
                           text-center"
                >

                    <p
                        class="text-sm
                               text-rose-400"
                    >
                        ${escapeHtml(message)}
                    </p>

                    <button
                        type="button"
                        class="mt-4 rounded-md
                               bg-slate-800
                               px-3 py-1.5
                               text-xs
                               text-slate-300
                               hover:bg-slate-700"
                        onclick="
                            window.location.href =
                            './collection_view.html'
                        "
                    >
                        Back to Collections
                    </button>

                </td>

            </tr>

        `;
  }

  // ============================================================
  // HELPERS
  // ============================================================

  function getEndpointId(endpoint) {
    /*
     * Prefer the endpoint's own ID.
     * Fall back to the endpoint path so
     * older prototype data still works.
     */

    return String(endpoint.endpoint_id ?? endpoint.id ?? endpoint.endpoint ?? Math.random());
  }

  function getQPCount(endpoint) {
    if (Array.isArray(endpoint.queryParams)) {
      return endpoint.queryParams.length;
    }

    if (Array.isArray(endpoint.qp)) {
      return endpoint.qp.length;
    }

    if (endpoint.queryParams && typeof endpoint.queryParams === "object") {
      return Object.keys(endpoint.queryParams).length;
    }

    if (endpoint.qp && typeof endpoint.qp === "object") {
      return Object.keys(endpoint.qp).length;
    }

    return Number(endpoint.qpCount || 0);
  }

  function renderEndpointPath(path) {
    const segments = String(path || "")
      .split("/")
      .filter(Boolean);

    if (!segments.length) {
      return "—";
    }

    return segments
      .map((segment, index) => {
        return `

                        <span
                            class="endpoint-segment
                                   rounded px-0.5
                                   transition
                                   hover:bg-cyan-500/10
                                   hover:text-cyan-300"
                            data-segment="${escapeAttr(segment)}"
                        >
                            ${
                              index === 0
                                ? "/" + escapeHtml(segment)
                                : "/" + escapeHtml(segment)
                            }
                        </span>

                    `;
      })
      .join("");
  }

  function methodClass(method) {
    return (
      {
        GET: "text-emerald-400",

        POST: "text-sky-400",

        PUT: "text-amber-400",

        PATCH: "text-violet-400",

        DELETE: "text-rose-400",
      }[method] || "text-slate-300"
    );
  }

  function capitalize(value) {
    const text = String(value || "");

    return text ? text.charAt(0).toUpperCase() + text.slice(1) : "";
  }

  function formatData(value) {
    if (typeof value === "string") {
      return value;
    }

    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  // ============================================================
  // CONTEXT MENU
  // ============================================================

  function openEndpointContextMenu(event, endpoint) {
    if (!contextMenu) return;

        const count = state.selected.size;

        let menuHtml = `
            <div class="px-3 py-2 text-[10px] font-semibold tracking-wide text-slate-500 uppercase border-b border-slate-800">
                ${count > 1 ? `${count} Endpoints Selected` : "Endpoint Options"}
            </div>
            
            <div class="p-1">
        `;

        if (count === 1) {
          menuHtml += `
                <button type="button" id="ctxEditTags" class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-slate-300 hover:bg-slate-800">
                    <svg class="h-3.5 w-3.5 text-slate-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                        <path d="M20.5 13.5 13.5 20.5 a2 2 0 0 1-2.8 0L4 13.8V4h9.8l6.7 6.7 a2 2 0 0 1 0 2.8Z" />
                        <circle cx="8.5" cy="8.5" r="1" />
                    </svg>
                    Modify
                </button>
            `;
        }

        menuHtml += `
                <button type="button" id="ctxManageTags" class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-slate-300 hover:bg-slate-800">
                    <svg class="h-3.5 w-3.5 text-slate-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                        <path d="M20.5 13.5 13.5 20.5 a2 2 0 0 1-2.8 0L4 13.8V4h9.8l6.7 6.7 a2 2 0 0 1 0 2.8Z" />
                        <circle cx="8.5" cy="8.5" r="1" />
                    </svg>
                    Manage Tags (Bulk)
                </button>
            </div>
            
            <div class="border-t border-slate-800 p-1">
                <button type="button" id="ctxRemoveEndpoint" class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-rose-400 hover:bg-rose-500/10 hover:text-rose-300">
                    <svg class="h-3.5 w-3.5 opacity-70" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                        <path d="M4 7h16" />
                        <path d="M10 11v6M14 11v6" />
                        <path d="M6 7l1 13h10l1-13" />
                        <path d="M9 7V4h6v3" />
                    </svg>
                    Remove from Collection
                </button>
            </div>
        `;

        contextMenu.innerHTML = menuHtml;
        contextMenu.classList.remove("hidden");

        // Position menu
        const rect = contextMenu.getBoundingClientRect();
        let x = event.clientX;
        let y = event.clientY;

        if (x + rect.width > window.innerWidth)
          x = window.innerWidth - rect.width - 5;
        if (y + rect.height > window.innerHeight)
          y = window.innerHeight - rect.height - 5;

        contextMenu.style.left = `${x}px`;
        contextMenu.style.top = `${y}px`;

        // Bind events
        if (count === 1) {
          document
            .getElementById("ctxEditTags")
            ?.addEventListener("click", () => {
              closeContextMenu();
              openTagEditor(endpoint);
            });
        }

        document
          .getElementById("ctxManageTags")
          .addEventListener("click", () => {
            closeContextMenu();
            const btn = document.getElementById("tagsManager");
            if (btn) btn.click();
          });

        document
          .getElementById("ctxRemoveEndpoint")
          .addEventListener("click", () => {
            closeContextMenu();
            const btn = document.getElementById("deleteSelected");
            if (btn) btn.click();
          });
      }

      function openTagEditor(endpoint) {
        const currentTags = Array.isArray(endpoint.tags)
          ? endpoint.tags.join(", ")
          : "";
        const currentAnnotation = endpoint.annotation || "";

        const endpointId = getEndpointId(endpoint);
        const pathText = endpoint.endpoint ? ` — ${escapeHtml(endpoint.endpoint)}` : "";

        openModal(
          `Endpoint — ${escapeHtml(endpointId)}${pathText}`,
          `
                <div class="space-y-4">
                    <div>
                        <label class="mb-1 block text-xs text-slate-500">Tags</label>
                        <input id="tagEditorInput" value="${escapeAttr(currentTags)}" class="h-9 w-full rounded-md border border-slate-700 bg-slate-950 px-3 text-xs outline-none focus:border-cyan-500">
                        <p class="mt-1 text-[10px] text-slate-600">Separate tags with commas.</p>
                    </div>
                    <div>
                        <label class="mb-1 block text-xs text-slate-500">Annotation</label>
                        <textarea id="annotationEditorInput" class="h-20 w-full rounded-md border border-slate-700 bg-slate-950 p-2 text-xs outline-none focus:border-cyan-500" placeholder="Optional notes...">${escapeHtml(currentAnnotation)}</textarea>
                    </div>
                    <div class="flex justify-end gap-2">
                        <button type="button" data-modal-close class="rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-400 hover:bg-slate-800">Cancel</button>
                        <button id="saveTagsBtn" type="button" class="rounded-md bg-cyan-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-cyan-500">Save</button>
                    </div>
                </div>
            `,
        );

        document
          .getElementById("saveTagsBtn")
          ?.addEventListener("click", async () => {
            const btn = document.getElementById("saveTagsBtn");
            const input = document.getElementById("tagEditorInput");
            const annotationInput = document.getElementById("annotationEditorInput");

            btn.disabled = true;
            btn.textContent = "Saving...";

            const newTags = input.value
              .split(",")
              .map((tag) => tag.trim())
              .filter(Boolean);
            const oldTags = Array.isArray(endpoint.tags)
              ? [...endpoint.tags]
              : [];
            const oldTagSet = new Set(oldTags);
            const newTagSet = new Set(newTags);

            const tagsToAdd = newTags.filter((tag) => !oldTagSet.has(tag));
            const tagsToRemove = oldTags.filter((tag) => !newTagSet.has(tag));

            const newAnnotation = annotationInput.value;
            const isAnnotationChanged = newAnnotation !== currentAnnotation;

            const api = window.EdmsAPI;
            if (!api || typeof api.addEndpointTag !== "function") {
              console.error("API unavailable");
              return;
            }

            try {
              for (const tag of tagsToRemove) {
                await api.removeEndpointTag(getEndpointId(endpoint), tag);
              }
              for (const tag of tagsToAdd) {
                await api.addEndpointTag(getEndpointId(endpoint), tag);
              }

              if (isAnnotationChanged && typeof api.setEndpointAnnotation === "function") {
                await api.setEndpointAnnotation(getEndpointId(endpoint), newAnnotation);
                endpoint.annotation = newAnnotation;
              }

              // Instead of completely reloading, update local state
              endpoint.tags = newTags;
              renderTable();
              closeModal();
            } catch (err) {
              console.error(err);
              btn.disabled = false;
              btn.textContent = "Error";
            }
          });
      }

      function wireRemoveAction() {
        const delBtn = document.getElementById("deleteSelected");
        if (delBtn) {
          delBtn.addEventListener("click", () => {
            if (state.selected.size === 0) return;

            openModal(
              "Remove Endpoints",
              `
                    <div class="space-y-4">
                        <p class="text-xs text-slate-300">
                            Are you sure you want to remove ${state.selected.size} endpoint${state.selected.size === 1 ? "" : "s"} from this collection?
                        </p>
                        <p class="text-[10px] text-slate-500">
                            This will not delete the endpoints from the system, only from this collection.
                        </p>
                        <div class="flex justify-end gap-2 pt-2">
                            <button type="button" class="rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-400 hover:bg-slate-800" data-modal-close>Cancel</button>
                            <button id="confirmRemove" type="button" class="rounded-md bg-rose-600/90 px-3 py-1.5 text-xs font-medium text-white hover:bg-rose-500">Remove</button>
                        </div>
                    </div>
                `,
            );

            const confirmBtn = document.getElementById("confirmRemove");
            if (confirmBtn) {
              confirmBtn.addEventListener("click", async () => {
                confirmBtn.disabled = true;
                confirmBtn.textContent = "Removing...";

                const api = window.EdmsAPI;
                if (!api) return closeModal();

                const folderId = new URLSearchParams(
                  window.location.search,
                ).get("folder");

                try {
                  const ids = Array.from(state.selected);
                  for (const eid of ids) {
                    await api.removeCollectionEndpoint(folderId, eid);
                  }

                  // Re-fetch everything
                  await loadCollection();
                  state.selected.clear();
                  updateSelectionUI();
                  closeModal();
                } catch (e) {
                  console.error("Failed to remove endpoints:", e);
                  confirmBtn.disabled = false;
                  confirmBtn.textContent = "Error";
                }
              });
            }
          });
        }
      }

      // Call wireRemoveAction during init
      document.addEventListener("DOMContentLoaded", () => {
        // Will wait for init to finish before wiring to ensure DOM is ready
        setTimeout(wireRemoveAction, 100);
      });

  function escapeHtml(value) {
    const div = document.createElement("div");

    div.textContent = String(value ?? "");

    return div.innerHTML;
  }

  function escapeAttr(value) {
    return escapeHtml(value).replace(/"/g, "&quot;");
  }

  // ============================================================
  // MODAL AND CONTEXT MENU HELPERS
  // ============================================================

  const modalOverlay = document.getElementById("modalOverlay");
  const modalPanel = document.getElementById("modalPanel");
  const modalTitle = document.getElementById("modalTitle");
  const modalContent = document.getElementById("modalContent");
  const modalClose = document.getElementById("modalClose");
  const contextMenu = document.getElementById("contextMenu");

  if (modalClose) {
    modalClose.addEventListener("click", closeModal);
  }
  if (modalOverlay) {
    modalOverlay.addEventListener("click", (e) => {
      if (e.target === modalOverlay) closeModal();
    });
  }
  // Wire any [data-modal-close] buttons injected dynamically into the modal
  if (modalPanel) {
    modalPanel.addEventListener("click", (e) => {
      if (e.target.closest("[data-modal-close]")) closeModal();
    });
  }
  document.addEventListener("click", (e) => {
    if (contextMenu && !contextMenu.contains(e.target)) {
      closeContextMenu();
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      closeModal();
      closeContextMenu();
    }
  });

  function openModal(title, content) {
    if (!modalOverlay) return;
    modalTitle.textContent = title;
    modalContent.innerHTML = content;
    modalOverlay.classList.remove("hidden");
    modalOverlay.classList.add("flex");
  }

  function closeModal() {
    modalOverlay?.classList.add("hidden");
    modalOverlay?.classList.remove("flex");
  }

  function closeContextMenu() {
    contextMenu?.classList.add("hidden");
  }

  window.openModal = openModal;
  window.closeModal = closeModal;
  window.closeContextMenu = closeContextMenu;
})();
