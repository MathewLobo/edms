// =========================================
// EDMS TEST VIEW
// =========================================

// -----------------------------------------
// DATA
// -----------------------------------------

let endpoints = [];
let bookmarks = [];
let historyRecords = [];
let filteredTestEndpoints = [];

let selectedTestEndpoint = null;
let selectedTestQP = null;

let activeSidebarTab = "endpoints";
let activeTestMethod = "ALL";
let activeTimeFilter = "all";
let activeRequestTab = "headers";
let activeResponseTab = "headers";

let latestResponseMeta = null;
let runTimer = null;

const LOCAL_HISTORY_KEY = "edmsTestViewHistory";

const API_BASE = 'http://localhost:3000';


// =========================================
// DOM
// =========================================

const testEndpointList =
    document.getElementById("testEndpointList");

const testQPPanel =
    document.getElementById("qpPanel");

const testSearchInput =
    document.getElementById("testSearchInput");

const urlFilter =
    document.getElementById("urlFilter");

const timeFilter =
    document.getElementById("timeFilter");

const testMethod =
    document.getElementById("Method");

const baseUrl =
    document.getElementById("URL-prefix");

const endpointPath =
    document.getElementById("Endpoint-path");

const runButton =
    document.getElementById("runRequest");

const stopButton =
    document.getElementById("stopRequest");

const annotationInput =
    document.getElementById("annotations");

const tagInput =
    document.getElementById("tagInput");

const addTagButton =
    document.getElementById("addTagButton");

const endpointTags =
    document.getElementById("endpointTags");

const requestBox =
    document.getElementById("requestBox");

const responseBox =
    document.getElementById("responseBox");

const requestContent =
    document.getElementById("requestContent");

const responseContent =
    document.getElementById("responseContent");

// ===== ADDED: sidebar collapse + address bar mode DOM refs =====
const testSidebar =
    document.getElementById("testSidebar");

const sidebarCollapseToggle =
    document.getElementById("sidebarCollapseToggle");

const addressModeToggle =
    document.getElementById("addressModeToggle");

const addressSplit =
    document.getElementById("addressSplit");

const urlFullInput =
    document.getElementById("URL-full");
// ===== END ADDED =====


// =========================================
// INIT
// =========================================

document.addEventListener(
    "DOMContentLoaded",
    initTestView
);


async function initTestView() {

    await loadTestData();

    setupTestSearch();

    setupMethodFilters();

    setupTimeFilter();

    setupURLFilter();

    setupRunner();

    setupTags();

    setupTabs();

    setupPanelControls();

    setupSidebarTabs();

    setupSidebarCollapse(); // ADDED

    setupAddressMode();     // ADDED

    applyTestFilters();

}


// =========================================
// LOAD TEST DATA
// =========================================

async function loadTestData() {

    const [
        endpointData,
        bookmarkData,
        historyData
    ] = await Promise.all([
        fetchJSON("../data/endpoints.json"),
        fetchJSON("../data/bookmarks.json"),
        fetchJSON("../data/history.json")
    ]);

    endpoints =
        Array.isArray(endpointData.endpoints)
            ? endpointData.endpoints
            : [];

    bookmarks =
        Array.isArray(bookmarkData.bookmarks)
            ? bookmarkData.bookmarks
            : [];

    const fileHistory =
        Array.isArray(historyData.history)
            ? historyData.history
            : [];

    historyRecords = [
        ...loadLocalHistory(),
        ...fileHistory
    ];

}


async function fetchJSON(path) {

    try {

        const response =
            await fetch(path);

        if (!response.ok) {

            throw new Error(
                `${response.status} ${response.statusText}`
            );

        }

        return await response.json();

    }

    catch (error) {

        console.error(
            `Failed to load ${path}:`,
            error
        );

        return {};

    }

}


// =========================================
// SIDEBAR DATA
// =========================================

function getActiveSidebarItems() {

    if (activeSidebarTab === "history") {

        return historyRecords.map(history => {

            const endpoint =
                findEndpoint(history.endpointId);

            return {
                type: "history",
                id: history.id,
                endpointId: history.endpointId,
                qpId: history.qpId,
                method: history.method || endpoint?.method || "",
                endpoint: history.endpoint || endpoint?.endpoint || "",
                updated: history.testedAt,
                status: history.status,
                saved: history.saved,
                source: history,
                endpointRef: endpoint
            };

        });

    }


    if (activeSidebarTab === "bookmarks") {

        return bookmarks.map(bookmark => {

            const endpoint =
                findEndpoint(bookmark.endpointId);

            return {
                type: "bookmark",
                id: bookmark.id,
                endpointId: bookmark.endpointId,
                qpId: bookmark.qpId,
                method: endpoint?.method || "",
                endpoint: endpoint?.endpoint || "",
                updated: bookmark.bookmarkedAt,
                saved: true,
                source: bookmark,
                endpointRef: endpoint
            };

        }).filter(item => item.endpointRef);

    }


    return endpoints.map(endpoint => ({
        type: "endpoint",
        id: endpoint.id,
        endpointId: endpoint.id,
        method: endpoint.method,
        endpoint: endpoint.endpoint,
        updated: endpoint.updated || endpoint.addedAt,
        saved: endpoint.bookmarked,
        source: endpoint,
        endpointRef: endpoint
    }));

}


// =========================================
// RENDER ENDPOINT LIST
// =========================================

function renderTestEndpoints() {

    if (!testEndpointList) return;

    testEndpointList.innerHTML = "";

    if (filteredTestEndpoints.length === 0) {

        testEndpointList.innerHTML = `
            <div class="p-4 text-sm text-slate-500">
                No ${activeSidebarTab} found.
            </div>
        `;

        return;

    }


    filteredTestEndpoints.forEach(item => {

        const card =
            createTestEndpointCard(item);

        testEndpointList.appendChild(card);

    });

}


// =========================================
// CREATE ENDPOINT CARD
// =========================================

function createTestEndpointCard(item) {

    const card =
        document.createElement("div");

    const endpoint =
        item.endpointRef || item.source || item;


    card.className = `
        endpoint-card
        group
        rounded-lg
        border
        border-slate-800
        bg-slate-900
        hover:bg-slate-800/80
        hover:border-cyan-500/50
        transition-all
        duration-200
        cursor-pointer
        px-3
        py-3
        hover:-translate-y-0.5
    `;


    card.dataset.id =
        item.id || endpoint.id;

    card.dataset.endpointId =
        item.endpointId || endpoint.id;


    const methodColor =
        getMethodColor(item.method || endpoint.method);

    const badge =
        getItemBadge(item);


    card.innerHTML = `

        <div class="flex items-center justify-between">

            <span
                class="px-2
                py-1
                rounded-md
                text-[11px]
                font-semibold
                border
                ${methodColor}">
                ${escapeHTML(item.method || endpoint.method || "")}
            </span>

            <span
                class="text-[11px]
                text-slate-500
                group-hover:text-slate-300
                transition-colors
                duration-200">
                ${escapeHTML(formatEndpointDate(item.updated))}
            </span>

        </div>


        <p
            class="mt-2
            text-sm
            font-medium
            text-slate-200
            truncate
            group-hover:text-white">
            ${escapeHTML(item.endpoint || endpoint.endpoint || "")}
        </p>

        <div
            class="mt-2
            flex
            items-center
            justify-between
            text-[11px]
            text-slate-500">

            <span>${escapeHTML(badge)}</span>
            <span>${getQPCountLabel(endpoint)}</span>

        </div>

    `;


    card.addEventListener(
        "click",
        () => {

            selectSidebarItem(
                item,
                card
            );

        }
    );


    return card;

}


function getMethodColor(method) {

    switch (method) {

        case "GET":
            return "bg-emerald-500/15 text-emerald-400 border-emerald-500/20";

        case "POST":
            return "bg-amber-500/15 text-amber-400 border-amber-500/20";

        case "PUT":
            return "bg-sky-500/15 text-sky-400 border-sky-500/20";

        case "DELETE":
            return "bg-red-500/15 text-red-400 border-red-500/20";

        default:
            return "bg-slate-700 text-slate-300 border-slate-600";

    }

}


function getItemBadge(item) {

    if (item.type === "history") {

        const status =
            item.status
                ? `${item.status} ${getStatusText(item.status)}`
                : "Tested";

        return item.saved
            ? `${status} - Saved`
            : `${status} - Not saved`;

    }


    if (item.type === "bookmark") {

        return "Bookmarked";

    }


    return item.saved
        ? "Added - Bookmarked"
        : "Added";

}


function getQPCountLabel(endpoint) {

    const count =
        Array.isArray(endpoint?.qps)
            ? endpoint.qps.length
            : 0;

    return `${count} QP${count === 1 ? "" : "s"}`;

}


// =========================================
// SELECT ENDPOINT / QP
// =========================================

function selectSidebarItem(
    item,
    card
) {

    const endpoint =
        item.endpointRef || item.source || item;

    if (!endpoint) return;

    selectTestEndpoint(
        endpoint,
        card,
        item.qpId
    );

    if (item.type === "history") {

        latestResponseMeta = {
            elapsed: item.source?.elapsedMs,
            testedAt: item.source?.testedAt
        };

        renderCurrentResponse();

    }

}


function selectTestEndpoint(
    endpoint,
    card,
    preferredQPId
) {

    selectedTestEndpoint =
        endpoint;

    latestResponseMeta =
        null;


    document
        .querySelectorAll(".endpoint-card")
        .forEach(item => {

            item.classList.remove(
                "bg-sky-500/10",
                "border-l-2",
                "border-sky-500"
            );

        });


    if (card) {

        card.classList.add(
            "bg-sky-500/10",
            "border-l-2",
            "border-sky-500"
        );

    }


    if (testMethod) {

        testMethod.value =
            endpoint.method;

    }


    if (baseUrl) {

        baseUrl.value =
            endpoint.baseUrl || "https://api.edms.local";

    }


    if (endpointPath) {

        endpointPath.value =
            endpoint.endpoint;

    }


    // ADDED: keep the combined address input in sync
    syncAddressFullDisplay();


    if (annotationInput) {

        annotationInput.value =
            endpoint.annotation || "";

    }


    renderSelectedEndpointTags();

    renderTestQP(
        endpoint,
        preferredQPId
    );

}


// =========================================
// RENDER QP
// =========================================

function renderTestQP(
    endpoint,
    preferredQPId
) {

    if (!testQPPanel) return;

    testQPPanel.innerHTML = "";

    selectedTestQP =
        null;


    if (!Array.isArray(endpoint.qps) || endpoint.qps.length === 0) {

        testQPPanel.innerHTML = `
            <div class="text-center text-xs text-slate-500">
                -
            </div>
        `;

        clearRequestResponse();

        return;

    }


    let preferredButton = null;
    let preferredQP = null;


    endpoint.qps.forEach(qp => {

        const button =
            document.createElement("button");


        button.className = `
            qp-btn
            w-full
            min-h-10
            rounded-md
            border
            border-slate-700
            bg-slate-800
            px-1
            py-2
            text-xs
            text-slate-300
            hover:bg-cyan-500
            hover:text-white
            transition-all
            duration-150
            hover:scale-105
            active:scale-95
        `;


        button.textContent =
            qp.id;

        button.title =
            qp.name || `QP ${qp.id}`;

        button.dataset.qp =
            qp.id;


        button.addEventListener(
            "click",
            () => {

                selectTestQP(
                    qp,
                    button
                );

            }
        );


        if (
            String(qp.id) ===
            String(preferredQPId || endpoint.qps[0].id)
        ) {

            preferredButton =
                button;

            preferredQP =
                qp;

        }


        testQPPanel.appendChild(
            button
        );

    });


    selectTestQP(
        preferredQP || endpoint.qps[0],
        preferredButton || testQPPanel.querySelector(".qp-btn")
    );

}


// =========================================
// SELECT QP
// =========================================

function selectTestQP(
    qp,
    button
) {

    selectedTestQP =
        qp;

    latestResponseMeta =
        null;


    document
        .querySelectorAll(".qp-btn")
        .forEach(item => {

            item.classList.remove(
                "bg-cyan-500",
                "text-white"
            );

            item.classList.add(
                "bg-slate-800",
                "text-slate-300"
            );

        });


    if (button) {

        button.classList.remove(
            "bg-slate-800",
            "text-slate-300"
        );

        button.classList.add(
            "bg-cyan-500",
            "text-white"
        );

    }


    renderCurrentRequest();

    renderCurrentResponse();

}


// =========================================
// REQUEST / RESPONSE PREVIEW
// =========================================

function renderCurrentRequest() {

    if (!requestContent) return;

    if (!selectedTestQP) {

        requestContent.value =
            "";

        return;

    }


    const request =
        selectedTestQP.request || {};

    const content =
        activeRequestTab === "headers"
            ? request.headers || {}
            : getRequestBodyPreview(request);

    requestContent.value =
        formatJSON(content);

}


function renderCurrentResponse() {

    if (!responseContent) return;

    if (!selectedTestQP) {

        responseContent.value =
            "";

        return;

    }


    const response =
        selectedTestQP.response || {};

    const content =
        activeResponseTab === "headers"
            ? getResponseHeadersPreview(response)
            : response.body || {};

    responseContent.value =
        formatJSON(content);

}


function getRequestBodyPreview(request) {

    const bodyPreview = {};

    ["path", "query", "body"].forEach(key => {

        if (request[key] !== undefined) {

            bodyPreview[key] =
                request[key];

        }

    });

    return Object.keys(bodyPreview).length
        ? bodyPreview
        : {};

}


function getResponseHeadersPreview(response) {

    const status =
        response.status || 200;

    const headers = {
        status: `${status} ${getStatusText(status)}`.trim(),
        ...(response.headers || {})
    };

    if (latestResponseMeta?.elapsed !== undefined) {

        headers.time =
            `${latestResponseMeta.elapsed} ms`;

    }

    if (latestResponseMeta?.testedAt) {

        headers.testedAt =
            latestResponseMeta.testedAt;

    }

    return headers;

}


function clearRequestResponse() {

    if (requestContent) {

        requestContent.value =
            "";

    }

    if (responseContent) {

        responseContent.value =
            "";

    }

}


function formatJSON(value) {

    return JSON.stringify(
        value || {},
        null,
        4
    );

}


// =========================================
// SEARCH / FILTERS
// =========================================

function setupTestSearch() {

    if (!testSearchInput) return;

    testSearchInput.addEventListener(
        "input",
        applyTestFilters
    );

}


function setupMethodFilters() {

    const methods = [
        "GET",
        "POST",
        "PUT",
        "DELETE"
    ];


    methods.forEach(method => {

        const button =
            document.getElementById(
                `method${method}`
            );

        if (!button) return;

        button.addEventListener(
            "click",
            () => {

                activeTestMethod =
                    activeTestMethod === method
                        ? "ALL"
                        : method;

                updateMethodButtons();

                applyTestFilters();

            }
        );

    });

}


function updateMethodButtons() {

    [
        "GET",
        "POST",
        "PUT",
        "DELETE"
    ].forEach(method => {

        const button =
            document.getElementById(
                `method${method}`
            );

        if (!button) return;

        button.classList.remove(
            "ring-2",
            "ring-cyan-400"
        );

        if (activeTestMethod === method) {

            button.classList.add(
                "ring-2",
                "ring-cyan-400"
            );

        }

    });

}


function setupTimeFilter() {

    if (!timeFilter) return;

    timeFilter.addEventListener(
        "change",
        () => {

            activeTimeFilter =
                timeFilter.value;

            applyTestFilters();

        }
    );

}


function setupURLFilter() {

    if (!urlFilter) return;

    urlFilter.addEventListener(
        "input",
        applyTestFilters
    );

}


function applyTestFilters() {

    const search =
        testSearchInput
            ? testSearchInput.value
                .trim()
                .toLowerCase()
            : "";

    const url =
        urlFilter
            ? urlFilter.value
                .trim()
                .toLowerCase()
            : "";


    filteredTestEndpoints =
        getActiveSidebarItems().filter(item => {

            const endpoint =
                item.endpointRef || item.source || item;

            const haystack =
                [
                    item.id,
                    item.endpointId,
                    item.method,
                    item.endpoint,
                    item.status,
                    item.source?.qpName,
                    endpoint?.annotation,
                    ...(endpoint?.tags || [])
                ].join(" ").toLowerCase();


            const matchesSearch =
                !search ||
                haystack.includes(search);

            const matchesMethod =
                activeTestMethod === "ALL" ||
                item.method === activeTestMethod;

            const matchesURL =
                !url ||
                String(item.endpoint || "")
                    .toLowerCase()
                    .includes(url);

            const matchesTime =
                matchesTimeFilter(item.updated);

            return (
                matchesSearch &&
                matchesMethod &&
                matchesURL &&
                matchesTime
            );

        });


    renderTestEndpoints();

}


function matchesTimeFilter(
    dateString
) {

    if (activeTimeFilter === "all") {

        return true;

    }

    if (!dateString) {

        return false;

    }

    const itemDate =
        new Date(dateString);

    if (Number.isNaN(itemDate.getTime())) {

        return false;

    }

    const now =
        new Date();

    const oneDay =
        24 * 60 * 60 * 1000;

    const difference =
        now - itemDate;


    switch (activeTimeFilter) {

        case "today":
            return (
                itemDate.toDateString() ===
                now.toDateString()
            );

        case "week":
        case "7days":
            return (
                difference >= 0 &&
                difference <= 7 * oneDay
            );

        case "30days":
            return (
                difference >= 0 &&
                difference <= 30 * oneDay
            );

        default:
            return true;

    }

}


// =========================================
// RUN / STOP
// =========================================

function setupRunner() {

    if (runButton) {

        runButton.addEventListener(
            "click",
            runTestEndpoint
        );

    }


    if (stopButton) {

        stopButton.addEventListener(
            "click",
            stopTestEndpoint
        );

    }

}


function runTestEndpoint() {

    if (!selectedTestEndpoint) {

        window.alert("Select an endpoint first.");

        return;

    }


    if (!selectedTestQP) {

        window.alert("Select a QP first.");

        return;

    }


    clearTimeout(runTimer);

    setRunButtonState(true);

    const startTime =
        performance.now();


    runTimer =
        setTimeout(() => {

            const elapsed =
                Math.round(
                    performance.now() -
                    startTime
                );

            latestResponseMeta = {
                elapsed,
                testedAt: new Date().toISOString()
            };

            renderCurrentResponse();

            addLocalHistoryRecord(
                elapsed
            );

            setRunButtonState(false);

            runTimer =
                null;

        }, 600);

}


async function stopTestEndpoint() {

    if (runTimer) {
        clearTimeout(runTimer);
        runTimer = null;
        setRunButtonState(false);

        if (responseContent) {
            responseContent.value = formatJSON({
                status: "Stopped",
                message: "The prototype request was stopped before completion."
            });
        }
    }

    try {
        const response = await fetch(`${API_BASE}/test-view/stop`, { method: 'POST' });
        console.log('Stop call status:', response.status);
    } catch (error) {
        console.error('Stop call failed:', error);
    }

}


function setRunButtonState(isRunning) {

    if (!runButton) return;

    runButton.disabled =
        isRunning;

    runButton.classList.toggle(
        "opacity-50",
        isRunning
    );

}


// =========================================
// LOCAL HISTORY
// =========================================

function addLocalHistoryRecord(
    elapsed
) {

    const response =
        selectedTestQP.response || {};

    const record = {
        id: `HL-${Date.now()}`,
        endpointId: selectedTestEndpoint.id,
        method: selectedTestEndpoint.method,
        endpoint: selectedTestEndpoint.endpoint,
        qpId: selectedTestQP.id,
        qpName: selectedTestQP.name,
        testedAt: new Date().toISOString(),
        status: response.status || 200,
        elapsedMs: elapsed,
        saved: Boolean(selectedTestEndpoint.bookmarked)
    };

    const localHistory =
        loadLocalHistory();

    localHistory.unshift(record);

    localStorage.setItem(
        LOCAL_HISTORY_KEY,
        JSON.stringify(localHistory.slice(0, 50))
    );

    historyRecords.unshift(record);

    if (activeSidebarTab === "history") {

        applyTestFilters();

    }

}


function loadLocalHistory() {

    try {

        const saved =
            JSON.parse(
                localStorage.getItem(LOCAL_HISTORY_KEY) || "[]"
            );

        return Array.isArray(saved)
            ? saved
            : [];

    }

    catch (error) {

        console.error(
            "Failed to read local history:",
            error
        );

        return [];

    }

}


// =========================================
// STATUS TEXT
// =========================================

function getStatusText(
    status
) {

    const statusTexts = {
        200: "OK",
        201: "Created",
        204: "No Content",
        400: "Bad Request",
        401: "Unauthorized",
        403: "Forbidden",
        404: "Not Found",
        409: "Conflict",
        422: "Validation Error",
        500: "Server Error"
    };

    return (
        statusTexts[status] ||
        ""
    );

}


// =========================================
// TAGS
// =========================================

function setupTags() {

    if (addTagButton) {

        addTagButton.addEventListener(
            "click",
            addTestTag
        );

    }


    if (tagInput) {

        tagInput.addEventListener(
            "keydown",
            event => {

                if (event.key === "Enter") {

                    event.preventDefault();

                    addTestTag();

                }

            }
        );

    }


    if (annotationInput) {

        annotationInput.addEventListener(
            "input",
            () => {

                if (!selectedTestEndpoint) return;

                selectedTestEndpoint.annotation =
                    annotationInput.value;

            }
        );

    }

}


function addTestTag() {

    if (!selectedTestEndpoint) return;

    const tag =
        tagInput
            ? tagInput.value.trim()
            : "";

    if (!tag) return;

    if (!Array.isArray(selectedTestEndpoint.tags)) {

        selectedTestEndpoint.tags =
            [];

    }

    if (!selectedTestEndpoint.tags.includes(tag)) {

        selectedTestEndpoint.tags.push(tag);

    }

    if (tagInput) {

        tagInput.value =
            "";

    }

    renderSelectedEndpointTags();

}


function renderSelectedEndpointTags() {

    if (!endpointTags) return;

    endpointTags.innerHTML =
        "";

    if (
        !selectedTestEndpoint ||
        !Array.isArray(selectedTestEndpoint.tags) ||
        selectedTestEndpoint.tags.length === 0
    ) {

        endpointTags.innerHTML = `
            <span class="text-xs text-slate-500">
                No tags selected.
            </span>
        `;

        return;

    }


    selectedTestEndpoint.tags.forEach(tag => {

        const tagElement =
            document.createElement("span");

        tagElement.className = `
            px-3
            py-1
            rounded-full
            bg-cyan-500/15
            text-cyan-300
            text-xs
            border
            border-cyan-500/20
        `;

        tagElement.textContent =
            tag;

        endpointTags.appendChild(
            tagElement
        );

    });

}


// =========================================
// REQUEST / RESPONSE TABS
// =========================================

function setupTabs() {

    setupContentTab(
        "requestHeadersTab",
        "request",
        "headers"
    );

    setupContentTab(
        "requestBodyTab",
        "request",
        "body"
    );

    setupContentTab(
        "responseHeadersTab",
        "response",
        "headers"
    );

    setupContentTab(
        "responseBodyTab",
        "response",
        "body"
    );

    updateContentTabButtons(
        "request",
        "headers"
    );

    updateContentTabButtons(
        "response",
        "headers"
    );

}


function setupContentTab(
    buttonId,
    panel,
    tab
) {

    const button =
        document.getElementById(buttonId);

    if (!button) return;

    button.addEventListener(
        "click",
        () => {

            if (panel === "request") {

                activeRequestTab =
                    tab;

                renderCurrentRequest();

            }

            else {

                activeResponseTab =
                    tab;

                renderCurrentResponse();

            }

            updateContentTabButtons(
                panel,
                tab
            );

        }
    );

}


function updateContentTabButtons(
    panel,
    activeTab
) {

    const ids =
        panel === "request"
            ? {
                headers: "requestHeadersTab",
                body: "requestBodyTab"
            }
            : {
                headers: "responseHeadersTab",
                body: "responseBodyTab"
            };

    Object.entries(ids).forEach(([tab, id]) => {

        const button =
            document.getElementById(id);

        if (!button) return;

        const isActive =
            tab === activeTab;

        button.classList.toggle(
            "bg-cyan-500",
            isActive
        );

        button.classList.toggle(
            "text-slate-950",
            isActive
        );

        button.classList.toggle(
            "font-semibold",
            isActive
        );

        button.classList.toggle(
            "bg-slate-800",
            !isActive
        );

        button.classList.toggle(
            "text-slate-300",
            !isActive
        );

        button.classList.toggle(
            "font-medium",
            !isActive
        );

    });

}


// =========================================
// PANEL CONTROLS
// =========================================

function setupPanelControls() {

    setupPanelControlSet(
        "request",
        requestBox,
        responseBox
    );

    setupPanelControlSet(
        "response",
        responseBox,
        requestBox
    );

}


function setupPanelControlSet(
    panelName,
    panel,
    siblingPanel
) {

    if (!panel) return;

    const closeButton =
        document.getElementById(`${panelName}Close`);

    const fullscreenButton =
        document.getElementById(`${panelName}Fullscreen`);

    const resetButton =
        document.getElementById(`${panelName}Reset`);


    if (closeButton) {

        closeButton.addEventListener(
            "click",
            () => {

                collapsePanel(panel);

            }
        );

    }


    if (fullscreenButton) {

        fullscreenButton.addEventListener(
            "click",
            () => {

                togglePanelFullscreen(
                    panel,
                    siblingPanel
                );

            }
        );

    }


    if (resetButton) {

        resetButton.addEventListener(
            "click",
            resetPanels
        );

    }

}


function collapsePanel(
    panel
) {

    panel.dataset.closed =
        "true";

    panel.style.minHeight =
        "auto";

    panel.style.height =
        "auto";

    Array.from(panel.children)
        .forEach(child => {

            if (!child.classList.contains("panelheader")) {

                child.style.display =
                    "none";

            }

        });

}


function togglePanelFullscreen(
    panel,
    siblingPanel
) {

    const isFullscreen =
        panel.dataset.fullscreen === "true";

    resetPanels();

    if (isFullscreen) return;

    panel.dataset.fullscreen =
        "true";

    panel.style.position =
        "fixed";

    panel.style.inset =
        "16px";

    panel.style.zIndex =
        "50";

    panel.style.height =
        "auto";

    panel.style.maxHeight =
        "calc(100vh - 32px)";

    panel.style.display =
        "flex";

    if (siblingPanel) {

        siblingPanel.style.display =
            "none";

    }

}


function resetPanels() {

    [
        requestBox,
        responseBox
    ].forEach(panel => {

        if (!panel) return;

        panel.dataset.fullscreen =
            "false";

        panel.dataset.closed =
            "false";

        panel.removeAttribute(
            "style"
        );

        Array.from(panel.children)
            .forEach(child => {

                child.style.display =
                    "";

            });

    });

}


// =========================================
// SIDEBAR TABS
// =========================================

function setupSidebarTabs() {

    setupSidebarTab(
        "historyTab",
        "history"
    );

    setupSidebarTab(
        "bookmarksTab",
        "bookmarks"
    );

    setupSidebarTab(
        "endpointsTab",
        "endpoints"
    );

    updateSidebarTabButtons();

}


function setupSidebarTab(
    buttonId,
    tab
) {

    const button =
        document.getElementById(buttonId);

    if (!button) return;

    button.addEventListener(
        "click",
        () => {

            activeSidebarTab =
                tab;

            selectedTestEndpoint =
                null;

            selectedTestQP =
                null;

            clearRequestResponse();

            updateSidebarTabButtons();

            applyTestFilters();

        }
    );

}


function updateSidebarTabButtons() {

    const tabs = {
        historyTab: "history",
        bookmarksTab: "bookmarks",
        endpointsTab: "endpoints"
    };

    Object.entries(tabs).forEach(([id, tab]) => {

        const button =
            document.getElementById(id);

        if (!button) return;

        const isActive =
            activeSidebarTab === tab;

        button.classList.toggle(
            "text-sky-400",
            isActive
        );

        button.classList.toggle(
            "border-sky-500",
            isActive
        );

        button.classList.toggle(
            "font-semibold",
            isActive
        );

        button.classList.toggle(
            "text-slate-400",
            !isActive
        );

        button.classList.toggle(
            "border-transparent",
            !isActive
        );

        button.classList.toggle(
            "font-medium",
            !isActive
        );

    });

}


// =========================================
// ADDED: SIDEBAR COLLAPSE
// =========================================

function setupSidebarCollapse() {

    if (!sidebarCollapseToggle || !testSidebar) return;

    const sidebarContent =
        document.getElementById("sidebarContent");

    const collapseIcon =
        sidebarCollapseToggle.querySelector(".icon-collapse");

    const expandIcon =
        sidebarCollapseToggle.querySelector(".icon-expand");

    sidebarCollapseToggle.addEventListener(
        "click",
        () => {

            const collapsed =
                testSidebar.classList.toggle("sidebar-collapsed");

            testSidebar.classList.toggle("w-[32%]", !collapsed);
            testSidebar.classList.toggle("w-12", collapsed);
            testSidebar.classList.toggle("min-w-12", collapsed);

            if (sidebarContent) {

                sidebarContent.classList.toggle("hidden", collapsed);

            }

            if (collapseIcon) collapseIcon.classList.toggle("hidden", collapsed);
            if (expandIcon) expandIcon.classList.toggle("hidden", !collapsed);

        }
    );

}


// =========================================
// ADDED: ADDRESS BAR MODE (combined / split)
// =========================================

let addressCombinedMode = false;

function setupAddressMode() {

    if (!addressModeToggle || !addressSplit || !urlFullInput) return;

    addressModeToggle.addEventListener(
        "click",
        () => {

            addressCombinedMode =
                !addressCombinedMode;

            if (addressCombinedMode) {

                syncAddressFullDisplay();

                addressSplit.classList.add("hidden");
                urlFullInput.classList.remove("hidden");

            } else {

                applyFullAddressToSplit();

                addressSplit.classList.remove("hidden");
                urlFullInput.classList.add("hidden");

            }

            addressModeToggle.classList.toggle("text-cyan-400", addressCombinedMode);
            addressModeToggle.classList.toggle("border-cyan-500/50", addressCombinedMode);

        }
    );

    urlFullInput.addEventListener(
        "input",
        applyFullAddressToSplit
    );

}


function syncAddressFullDisplay() {

    if (!urlFullInput) return;

    const prefix =
        baseUrl ? baseUrl.value.trim() : "";

    const path =
        endpointPath ? endpointPath.value.trim() : "";

    urlFullInput.value =
        `${prefix}${path}`;

}


function applyFullAddressToSplit() {

    if (!urlFullInput) return;

    const value =
        urlFullInput.value.trim();

    const match =
        value.match(/^(https?:\/\/[^/]+)(\/.*)?$/i);

    if (match) {

        if (baseUrl) baseUrl.value = match[1];
        if (endpointPath) endpointPath.value = match[2] || "";

    } else if (endpointPath) {

        endpointPath.value = value;

    }

}


// =========================================
// HELPERS
// =========================================

function findEndpoint(
    endpointId
) {

    return endpoints.find(endpoint => (
        String(endpoint.id) ===
        String(endpointId)
    ));

}


function formatEndpointDate(
    dateString
) {

    if (!dateString) return "";

    const date =
        new Date(dateString);

    if (Number.isNaN(date.getTime())) {

        return dateString;

    }

    const now =
        new Date();

    if (date.toDateString() === now.toDateString()) {

        return date.toLocaleTimeString(
            "en-US",
            {
                hour: "numeric",
                minute: "2-digit"
            }
        );

    }

    return date.toLocaleDateString(
        "en-GB",
        {
            day: "2-digit",
            month: "2-digit"
        }
    );

}


function escapeHTML(value) {

    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#039;");

}