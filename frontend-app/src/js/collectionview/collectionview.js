(() => {
    'use strict';

    // ============================================================
    // CONFIG
    // ============================================================

    const DATA_URL = '../data/collections.json';

    const PAGE_SIZE = 50;

    const API_BASE = 'http://localhost:3000';

    let nextId = 9000;


    // ============================================================
    // STATE
    // ============================================================

    const state = {

        folders: [],

        filtered: [],

        page: 1,

        search: '',

        purpose: 'all',

        tags: new Set(),

        segments: new Set(),

        selected: new Set(),

        activeFolderId: null,

        prevActiveFolderId: null,

        sidebarCollapsed: false

    };


    // ============================================================
    // INIT
    // ============================================================

    document.addEventListener(
        'DOMContentLoaded',
        init
    );


    async function init() {

        await loadFolders();

        wireToolbar();

        wireTableEvents();

        wireSidebar();

        wireSelection();

        wireModal();

        renderSidebar();

        applyFilters();

    }


    // ============================================================
    // DATA
    // ============================================================

    async function loadFolders() {

        try {

            const response =
                await fetch(DATA_URL);


            if (!response.ok) {

                throw new Error(
                    `HTTP ${response.status}`
                );

            }


            const data =
                await response.json();


            state.folders =
                Array.isArray(data.folders)
                    ? data.folders
                    : [];


            nextId =
                Math.max(
                    9000,
                    ...state.folders.map(
                        folder =>
                            Number(folder.id) || 0
                    )
                ) + 1;


            /*
             * Preserve the existing prototype behaviour:
             * second folder becomes active by default.
             */

            state.activeFolderId =
                state.folders[1]?.id ??
                state.folders[0]?.id ??
                null;


        } catch (error) {

            console.error(
                'Failed to load collections.json:',
                error
            );


            state.folders = [];

        }

    }


    // ============================================================
    // FILTERING
    // ============================================================

    function applyFilters() {

        const term =
            state.search
                .trim()
                .toLowerCase();


        state.filtered =
            state.folders.filter(
                folder => {

                    const endpoints =
                        Array.isArray(
                            folder.endpoints
                        )
                            ? folder.endpoints
                            : [];


                    const folderTags =
                        Array.isArray(
                            folder.tags
                        )
                            ? folder.tags
                            : [];


                    const matchesSearch =
                        !term ||

                        String(
                            folder.name || ''
                        )
                            .toLowerCase()
                            .includes(term) ||

                        String(
                            folder.annotation || ''
                        )
                            .toLowerCase()
                            .includes(term) ||

                        folderTags.some(
                            tag =>
                                String(tag)
                                    .toLowerCase()
                                    .includes(term)
                        ) ||

                        endpoints.some(
                            endpoint => {

                                const endpointPath =
                                    String(
                                        endpoint.endpoint ||
                                        ''
                                    )
                                        .toLowerCase();


                                const method =
                                    String(
                                        endpoint.method ||
                                        ''
                                    )
                                        .toLowerCase();


                                const endpointTags =
                                    Array.isArray(
                                        endpoint.tags
                                    )
                                        ? endpoint.tags
                                        : [];


                                return (
                                    endpointPath.includes(term) ||
                                    method.includes(term) ||
                                    endpointTags.some(
                                        tag =>
                                            String(tag)
                                                .toLowerCase()
                                                .includes(term)
                                    )
                                );

                            }
                        );


                    const matchesPurpose =
                        state.purpose === 'all' ||
                        folder.purpose ===
                        state.purpose;


                    const matchesTags =
                        state.tags.size === 0 ||
                        [...state.tags].every(
                            tag =>
                                folderTags.includes(tag) ||
                                endpoints.some(
                                    endpoint =>
                                        Array.isArray(
                                            endpoint.tags
                                        ) &&
                                        endpoint.tags.includes(tag)
                                )
                        );


                    const folderSegments =
                        getFolderSegments(
                            folder
                        );


                    const matchesSegments =
                        state.segments.size === 0 ||
                        [...state.segments].every(
                            segment =>
                                folderSegments.includes(
                                    segment
                                )
                        );


                    return (
                        matchesSearch &&
                        matchesPurpose &&
                        matchesTags &&
                        matchesSegments
                    );

                }
            );


        const totalPages =
            getTotalPages();


        if (
            state.page >
            totalPages
        ) {

            state.page =
                totalPages;

        }


        render();

    }


    function getTotalPages() {

        return Math.max(
            1,
            Math.ceil(
                state.filtered.length /
                PAGE_SIZE
            )
        );

    }


    function currentPageItems() {

        const start =
            (state.page - 1) *
            PAGE_SIZE;


        return state.filtered.slice(
            start,
            start + PAGE_SIZE
        );

    }


    // ============================================================
    // SEGMENTS
    // ============================================================

    function getEndpointSegments(
        endpointPath
    ) {

        return String(
            endpointPath || ''
        )
            .split('/')
            .filter(Boolean);

    }


    function getFolderSegments(
        folder
    ) {

        const segments =
            new Set();


        const endpoints =
            Array.isArray(
                folder.endpoints
            )
                ? folder.endpoints
                : [];


        endpoints.forEach(
            endpoint => {

                getEndpointSegments(
                    endpoint.endpoint
                ).forEach(
                    segment =>
                        segments.add(segment)
                );

            }
        );


        return [...segments];

    }


    // ============================================================
    // RENDER
    // ============================================================

    function render() {

        renderTable();

        renderPagination();

        renderActiveFolderIndicator();

        renderSelectionUI();

        updateStats();

        syncSidebar();

    }


    // ============================================================
    // TABLE
    // ============================================================

    function renderTable() {

        const tbody =
            document.getElementById(
                'folderTableBody'
            );


        if (!tbody) return;


        tbody.innerHTML = '';


        const items =
            currentPageItems();


        if (items.length === 0) {

            tbody.innerHTML = `

                <tr>

                    <td
                        colspan="9"
                        class="px-4 py-16 text-center"
                    >

                        <p
                            class="text-sm font-medium   
                                   text-slate-400"
                        >
                            No collections found
                        </p>

                        <p
                            class="mt-1 text-xs
                                   text-slate-600"
                        >
                            Try changing the current
                            search or filters.
                        </p>

                    </td>

                </tr>

            `;


            updateSelectAllState();

            return;

        }


        items.forEach(
            folder => {

                tbody.appendChild(
                    createFolderRow(folder)
                );

            }
        );


        updateSelectAllState();

    }


    function createFolderRow(
        folder
    ) {

        const row =
            document.createElement('tr');


        const id =
            Number(folder.id);


        const isSelected =
            state.selected.has(id);


        const isActive =
            Number(
                state.activeFolderId
            ) === id;


        const endpoints =
            Array.isArray(
                folder.endpoints
            )
                ? folder.endpoints
                : [];


        const counts =
            getCrudCounts(
                endpoints
            );


        row.dataset.id =
            String(id);


        row.className =
            [
                'folder-row',
                'border-b',
                'border-slate-800',
                'transition-colors',
                'duration-100',
                'cursor-pointer',
                'hover:bg-slate-800/60',

                isSelected
                    ? 'bg-cyan-500/[0.045]'
                    : '',

                isActive
                    ? 'bg-sky-500/[0.055]'
                    : ''
            ]
                .filter(Boolean)
                .join(' ');


        row.innerHTML = `

            <!-- Select -->

            <td class="px-2 py-2 align-middle">

                <input
                    type="checkbox"
                    class="folder-select
                           h-3.5 w-3.5
                           accent-cyan-400"
                    ${isSelected ? 'checked' : ''}
                >

            </td>


            <!-- Folder -->

            <td
                class="px-2 py-2
                       font-medium text-slate-200"
            >

                <div
                    class="flex min-w-0
                           items-center gap-2"
                >

                    ${isActive
                ? `
                                <span
                                    class="h-1.5 w-1.5
                                           shrink-0 rounded-full
                                           bg-cyan-400"
                                    title="Active collection"
                                ></span>
                              `
                : ''
            }

                    <span
                        class="truncate"
                        title="${escapeAttr(
                folder.name
            )}"
                    >
                        ${escapeHtml(
                folder.name
            )}
                    </span>

                </div>

            </td>


            

            <!-- Tags -->

            <td class="px-2 py-2">

                <div
                    class="flex flex-wrap
                           gap-1"
                >
                    ${renderFolderTags(
                folder
            )
            }
                </div>

            </td>


            <!-- GET -->

            <td class="px-1 py-2 text-center">
                ${crudCountBadge(
                counts.GET,
                'text-emerald-400'
            )}
            </td>


            <!-- POST -->

            <td class="px-1 py-2 text-center">
                ${crudCountBadge(
                counts.POST,
                'text-sky-400'
            )}
            </td>


            <!-- PUT -->

            <td class="px-1 py-2 text-center">
                ${crudCountBadge(
                counts.PUT,
                'text-amber-400'
            )}
            </td>


            <!-- PATCH -->

            <td class="px-1 py-2 text-center">
                ${crudCountBadge(
                counts.PATCH,
                'text-violet-400'
            )}
            </td>


            <!-- DELETE -->

            <td class="px-1 py-2 text-center">
                ${crudCountBadge(
                counts.DELETE,
                'text-rose-400'
            )}
            </td>


            <!-- Size -->

            <td
                class="px-2 py-2
                       text-slate-400"
            >
                ${calculateSize(folder).toFixed(1)} MB
            </td>


            <!-- Annotation -->

            <td class="px-2 py-2">

                <span
                    class="line-clamp-2
                           text-[11px]
                           text-slate-500"
                    title="${escapeAttr(
                folder.annotation || ''
            )}"
                >
                    ${escapeHtml(
                folder.annotation ||
                '—'
            )}
                </span>

            </td>

        `;


        // --------------------------------------------------------
        // Checkbox
        // --------------------------------------------------------

        const checkbox =
            row.querySelector(
                '.folder-select'
            );


        checkbox.addEventListener(
            'click',
            event => {

                event.stopPropagation();

            }
        );


        checkbox.addEventListener(
            'change',
            () => {

                if (
                    checkbox.checked
                ) {

                    state.selected.add(id);

                } else {

                    state.selected.delete(id);

                }


                renderSelectionUI();

                updateSelectAllState();

                renderTable();

            }
        );


        // --------------------------------------------------------
        // Tags
        // --------------------------------------------------------

        row.querySelectorAll(
            '.folder-tag'
        ).forEach(
            button => {

                button.addEventListener(
                    'click',
                    event => {

                        event.stopPropagation();

                        const tag =
                            button.dataset.tag;

                        state.tags.clear();

                        state.tags.add(
                            tag
                        );

                        state.page = 1;

                        applyFilters();

                    }
                );

            }
        );


        // --------------------------------------------------------
        // Row click
        // --------------------------------------------------------

        row.addEventListener(
            'click',
            event => {

                if (
                    event.target.closest(
                        'button,input'
                    )
                ) {

                    return;

                }


                openDataView(
                    id
                );

            }
        );


        // --------------------------------------------------------
        // Double click retained
        // --------------------------------------------------------

        row.addEventListener(
            'dblclick',
            event => {

                if (
                    event.target.closest(
                        'button,input'
                    )
                ) {

                    return;

                }


                openDataView(
                    id
                );

            }
        );


        return row;

    }


    function renderFolderTags(
        folder
    ) {

        const tags =
            Array.isArray(
                folder.tags
            )
                ? folder.tags
                : [];


        if (!tags.length) {

            return `
                <span
                    class="text-[10px]
                           text-slate-600"
                >
                    —
                </span>
            `;

        }


        return tags
            .map(
                tag => `

                    <button
                        type="button"
                        class="folder-tag
                               inline-flex
                               rounded
                               bg-sky-900/40
                               px-1.5 py-0.5
                               text-[10px]
                               text-sky-300
                               transition
                               hover:bg-sky-500/20
                               hover:text-sky-200"
                        data-tag="${escapeAttr(
                    tag
                )}"
                    >
                        ${escapeHtml(tag)}
                    </button>

                `
            )
            .join('');

    }


    function crudCountBadge(
        count,
        color
    ) {

        return `

            <span
                class="inline-flex min-w-6
                       items-center
                       justify-center
                       rounded-md bg-slate-800
                       px-1.5 py-0.5
                       text-[10px]
                       ${color}"
            >
                ${count}
            </span>

        `;

    }


    function getCrudCounts(
        endpoints
    ) {

        const counts = {

            GET: 0,

            POST: 0,

            PUT: 0,

            PATCH: 0,

            DELETE: 0

        };


        endpoints.forEach(
            endpoint => {

                const method =
                    String(
                        endpoint.method ||
                        ''
                    ).toUpperCase();


                if (
                    Object.prototype.hasOwnProperty.call(
                        counts,
                        method
                    )
                ) {

                    counts[method]++;

                }

            }
        );


        return counts;

    }


    // ============================================================
    // PAGINATION
    // ============================================================

    function renderPagination() {

        const total =
            state.filtered.length;


        const start =
            total
                ? (
                    (state.page - 1) *
                    PAGE_SIZE
                ) + 1
                : 0;


        const end =
            Math.min(
                state.page * PAGE_SIZE,
                total
            );


        document.getElementById(
            'paginationRangeStart'
        ).textContent =
            start;


        document.getElementById(
            'paginationRangeEnd'
        ).textContent =
            end;


        document.getElementById(
            'paginationTotal'
        ).textContent =
            total;


        const totalPages =
            getTotalPages();


        const numbers =
            document.getElementById(
                'paginationNumbers'
            );


        numbers.innerHTML = '';


        getPageRange(
            state.page,
            totalPages
        ).forEach(
            page => {

                if (
                    page === '...'
                ) {

                    const span =
                        document.createElement(
                            'span'
                        );


                    span.className =
                        'px-1 text-slate-600';


                    span.textContent =
                        '…';


                    numbers.appendChild(
                        span
                    );


                    return;

                }


                const button =
                    document.createElement(
                        'button'
                    );


                button.type =
                    'button';


                button.textContent =
                    page;


                button.className =
                    page === state.page

                        ? `
                            min-w-7 rounded-md
                            bg-cyan-500 px-2 py-1
                            text-[11px] text-white
                          `

                        : `
                            min-w-7 rounded-md
                            border border-slate-700
                            px-2 py-1
                            text-[11px]
                            text-slate-400
                            transition
                            hover:bg-slate-800
                            hover:text-white
                          `;


                button.addEventListener(
                    'click',
                    () => {

                        state.page =
                            page;

                        render();

                    }
                );


                numbers.appendChild(
                    button
                );

            }
        );


        const prev =
            document.getElementById(
                'paginationPrev'
            );


        const next =
            document.getElementById(
                'paginationNext'
            );


        prev.disabled =
            state.page <= 1;


        next.disabled =
            state.page >= totalPages;

    }


    function getPageRange(
        current,
        total
    ) {

        if (
            total <= 7
        ) {

            return Array.from(
                {
                    length: total
                },
                (_, index) =>
                    index + 1
            );

        }


        const pages = [
            1
        ];


        if (
            current > 4
        ) {

            pages.push(
                '...'
            );

        }


        const start =
            Math.max(
                2,
                current - 1
            );


        const end =
            Math.min(
                total - 1,
                current + 1
            );


        for (
            let page = start;
            page <= end;
            page++
        ) {

            pages.push(
                page
            );

        }


        if (
            current < total - 3
        ) {

            pages.push(
                '...'
            );

        }


        pages.push(
            total
        );


        return pages;

    }


    // ============================================================
    // SIDEBAR
    // ============================================================

    function wireSidebar() {

        document
            .getElementById(
                'sidebarToggle'
            )
            ?.addEventListener(
                'click',
                toggleSidebar
            );

    }


    function toggleSidebar() {

        const sidebar =
            document.getElementById(
                'filterSidebar'
            );


        const content =
            document.getElementById(
                'sidebarContent'
            );


        const title =
            document.getElementById(
                'sidebarTitle'
            );


        const icon =
            document.getElementById(
                'sidebarToggleIcon'
            );


        state.sidebarCollapsed =
            !state.sidebarCollapsed;


        if (
            state.sidebarCollapsed
        ) {

            sidebar.classList.remove(
                'w-48'
            );

            sidebar.classList.add(
                'w-10'
            );


            content.classList.add(
                'hidden'
            );


            title.classList.add(
                'hidden'
            );


            icon.innerHTML = `
                <path d="m9 18 6-6-6-6"/>
            `;

        } else {

            sidebar.classList.remove(
                'w-10'
            );

            sidebar.classList.add(
                'w-48'
            );


            content.classList.remove(
                'hidden'
            );


            title.classList.remove(
                'hidden'
            );


            icon.innerHTML = `
                <path d="m15 18-6-6 6-6"/>
            `;

        }

    }


    function renderSidebar() {

        renderTags();

        renderSegments();

        updateStats();

    }


    function renderTags() {

        const container =
            document.getElementById(
                'tagsContainer'
            );


        if (!container) return;


        const counts = {};


        state.folders.forEach(
            folder => {

                const tags =
                    Array.isArray(
                        folder.tags
                    )
                        ? folder.tags
                        : [];


                tags.forEach(
                    tag => {

                        if (!tag) return;

                        counts[tag] =
                            (
                                counts[tag] ||
                                0
                            ) + 1;

                    }
                );

            }
        );


        container.innerHTML = '';


        Object.entries(
            counts
        )
            .sort(
                (a, b) =>
                    a[0].localeCompare(
                        b[0]
                    )
            )
            .forEach(
                ([tag, count]) => {

                    const wrapper =
                        document.createElement(
                            'label'
                        );


                    wrapper.className =
                        'group flex cursor-pointer items-center gap-2';


                    wrapper.innerHTML = `

                        <input
                            type="checkbox"
                            class="collection-tag-checkbox
                                   h-3.5 w-3.5
                                   accent-cyan-400"
                            data-tag="${escapeAttr(
                        tag
                    )}"
                        >

                        <button
                            type="button"
                            class="collection-tag-filter
                                   min-w-0 flex-1
                                   truncate rounded
                                   px-1.5 py-1
                                   text-left text-[11px]
                                   text-slate-400
                                   transition
                                   hover:bg-sky-500/10
                                   hover:text-sky-300"
                            data-tag="${escapeAttr(
                        tag
                    )}"
                        >
                            ${escapeHtml(tag)}

                            <span
                                class="text-slate-600"
                            >
                                (${count})
                            </span>
                        </button>

                    `;


                    const checkbox =
                        wrapper.querySelector(
                            '.collection-tag-checkbox'
                        );


                    const button =
                        wrapper.querySelector(
                            '.collection-tag-filter'
                        );


                    checkbox.addEventListener(
                        'change',
                        () => {

                            setTagFilter(
                                tag,
                                checkbox.checked
                            );

                        }
                    );


                    button.addEventListener(
                        'click',
                        event => {

                            event.preventDefault();


                            const enabled =
                                !state.tags.has(
                                    tag
                                );


                            checkbox.checked =
                                enabled;


                            setTagFilter(
                                tag,
                                enabled
                            );

                        }
                    );


                    container.appendChild(
                        wrapper
                    );

                }
            );

    }


    function renderSegments() {

        const container =
            document.getElementById(
                'segmentsContainer'
            );


        if (!container) return;


        const counts = {};


        state.folders.forEach(
            folder => {

                getFolderSegments(
                    folder
                ).forEach(
                    segment => {

                        counts[segment] =
                            (
                                counts[segment] ||
                                0
                            ) + 1;

                    }
                );

            }
        );


        container.innerHTML = '';


        Object.entries(
            counts
        )
            .sort(
                (a, b) =>
                    a[0].localeCompare(
                        b[0]
                    )
            )
            .forEach(
                ([segment, count]) => {

                    const wrapper =
                        document.createElement(
                            'label'
                        );


                    wrapper.className =
                        'group flex cursor-pointer items-center gap-2';


                    wrapper.innerHTML = `

                        <input
                            type="checkbox"
                            class="collection-segment-checkbox
                                   h-3.5 w-3.5
                                   accent-cyan-400"
                            data-segment="${escapeAttr(
                        segment
                    )}"
                        >

                        <button
                            type="button"
                            class="collection-segment-filter
                                   min-w-0 flex-1
                                   truncate rounded
                                   px-1.5 py-1
                                   text-left text-[11px]
                                   text-slate-400
                                   transition
                                   hover:bg-cyan-500/10
                                   hover:text-cyan-300"
                            data-segment="${escapeAttr(
                        segment
                    )}"
                        >
                            ${escapeHtml(
                        segment
                    )}

                            <span
                                class="text-slate-600"
                            >
                                (${count})
                            </span>
                        </button>

                    `;


                    const checkbox =
                        wrapper.querySelector(
                            '.collection-segment-checkbox'
                        );


                    const button =
                        wrapper.querySelector(
                            '.collection-segment-filter'
                        );


                    checkbox.addEventListener(
                        'change',
                        () => {

                            setSegmentFilter(
                                segment,
                                checkbox.checked
                            );

                        }
                    );


                    button.addEventListener(
                        'click',
                        event => {

                            event.preventDefault();


                            const enabled =
                                !state.segments.has(
                                    segment
                                );


                            checkbox.checked =
                                enabled;


                            setSegmentFilter(
                                segment,
                                enabled
                            );

                        }
                    );


                    container.appendChild(
                        wrapper
                    );

                }
            );

    }


    function setTagFilter(
        tag,
        enabled
    ) {

        if (enabled) {

            state.tags.add(
                tag
            );

        } else {

            state.tags.delete(
                tag
            );

        }


        state.page = 1;

        applyFilters();

    }


    function setSegmentFilter(
        segment,
        enabled
    ) {

        if (enabled) {

            state.segments.add(
                segment
            );

        } else {

            state.segments.delete(
                segment
            );

        }


        state.page = 1;

        applyFilters();

    }


    function syncSidebar() {

        document
            .querySelectorAll(
                '.collection-tag-checkbox'
            )
            .forEach(
                checkbox => {

                    checkbox.checked =
                        state.tags.has(
                            checkbox.dataset.tag
                        );

                }
            );


        document
            .querySelectorAll(
                '.collection-segment-checkbox'
            )
            .forEach(
                checkbox => {

                    checkbox.checked =
                        state.segments.has(
                            checkbox.dataset.segment
                        );

                }
            );

    }


    // ============================================================
    // STATS
    // ============================================================

    function updateStats() {

        const folderStat =
            document.getElementById(
                'folderStat'
            );


        const endpointStat =
            document.getElementById(
                'endpointStat'
            );


        const tagStat =
            document.getElementById(
                'tagStat'
            );


        const segmentStat =
            document.getElementById(
                'segmentStat'
            );


        const tags =
            new Set();


        const segments =
            new Set();


        let endpointCount =
            0;


        state.folders.forEach(
            folder => {

                const folderTags =
                    Array.isArray(
                        folder.tags
                    )
                        ? folder.tags
                        : [];


                folderTags.forEach(
                    tag =>
                        tags.add(tag)
                );


                const endpoints =
                    Array.isArray(
                        folder.endpoints
                    )
                        ? folder.endpoints
                        : [];


                endpointCount +=
                    endpoints.length;


                endpoints.forEach(
                    endpoint => {

                        getEndpointSegments(
                            endpoint.endpoint
                        ).forEach(
                            segment =>
                                segments.add(
                                    segment
                                )
                        );

                    }
                );

            }
        );


        if (folderStat) {

            folderStat.textContent =
                state.folders.length;

        }


        if (endpointStat) {

            endpointStat.textContent =
                endpointCount;

        }


        if (tagStat) {

            tagStat.textContent =
                tags.size;

        }


        if (segmentStat) {

            segmentStat.textContent =
                segments.size;

        }

    }


    // ============================================================
    // ACTIVE FOLDER
    // ============================================================

    function renderActiveFolderIndicator() {

        const active =
            getFolder(
                state.activeFolderId
            );


        const previous =
            getFolder(
                state.prevActiveFolderId
            );


        const activeLabel =
            document.getElementById(
                'activeFolderLabel'
            );


        const previousLabel =
            document.getElementById(
                'prevActiveFolderLabel'
            );


        if (activeLabel) {

            activeLabel.textContent =
                active
                    ? active.name
                    : 'None';

        }


        if (previousLabel) {

            previousLabel.textContent =
                previous
                    ? `(prev: ${previous.name})`
                    : '';

        }

    }


    function setActiveFolder(
        id
    ) {

        const folder =
            getFolder(id);


        if (!folder) return;


        if (
            state.activeFolderId !==
            folder.id
        ) {

            state.prevActiveFolderId =
                state.activeFolderId;


            state.activeFolderId =
                folder.id;

        }


        render();

    }


    // ============================================================
    // SELECTION
    // ============================================================

    function wireSelection() {

        document
            .getElementById(
                'clearFolderSelection'
            )
            ?.addEventListener(
                'click',
                clearSelection
            );


        document
            .getElementById(
                'selectAllFolders'
            )
            ?.addEventListener(
                'change',
                event => {

                    currentPageItems()
                        .forEach(
                            folder => {

                                if (
                                    event.target.checked
                                ) {

                                    state.selected.add(
                                        Number(folder.id)
                                    );

                                } else {

                                    state.selected.delete(
                                        Number(folder.id)
                                    );

                                }

                            }
                        );


                    render();

                }
            );

    }


    function clearSelection() {

        state.selected.clear();

        render();

    }


    function renderSelectionUI() {

        const bar =
            document.getElementById(
                'selectionBar'
            );


        const count =
            document.getElementById(
                'selectedFolderCount'
            );


        const list =
            document.getElementById(
                'selectedFolderList'
            );


        const selected =
            getSelectedFolders();


        if (
            selected.length
        ) {

            bar?.classList.remove(
                'hidden'
            );

            bar?.classList.add(
                'flex'
            );

        } else {

            bar?.classList.add(
                'hidden'
            );

            bar?.classList.remove(
                'flex'
            );

        }


        if (count) {

            count.textContent =
                `${selected.length} selected`;

        }


        if (list) {

            list.innerHTML =
                selected
                    .map(
                        folder => `

                            <span
                                class="shrink-0
                                       rounded-md
                                       border
                                       border-cyan-500/20
                                       bg-cyan-500/10
                                       px-2 py-1
                                       font-mono
                                       text-[10px]
                                       text-cyan-300"
                            >
                                ${escapeHtml(
                            folder.name
                        )}
                            </span>

                        `
                    )
                    .join('');

        }

    }


    function updateSelectAllState() {

        const selectAll =
            document.getElementById(
                'selectAllFolders'
            );


        if (!selectAll) return;


        const items =
            currentPageItems();


        const selectedCount =
            items.filter(
                folder =>
                    state.selected.has(
                        Number(folder.id)
                    )
            ).length;


        selectAll.checked =
            items.length > 0 &&
            selectedCount ===
            items.length;


        selectAll.indeterminate =
            selectedCount > 0 &&
            selectedCount <
            items.length;

    }


    function getSelectedFolders() {

        return state.folders.filter(
            folder =>
                state.selected.has(
                    Number(folder.id)
                )
        );

    }


    // ============================================================
    // TOOLBAR
    // ============================================================

    function wireToolbar() {

        document
            .getElementById(
                'collectionSearch'
            )
            ?.addEventListener(
                'input',
                event => {

                    state.search =
                        event.target.value;

                    state.page = 1;

                    applyFilters();

                }
            );


        document
            .getElementById(
                'purposeFilter'
            )
            ?.addEventListener(
                'change',
                event => {

                    state.purpose =
                        event.target.value;

                    state.page = 1;

                    applyFilters();

                }
            );


        document
            .getElementById(
                'resetFilters'
            )
            ?.addEventListener(
                'click',
                resetFilters
            );


        document
            .getElementById(
                'newCollectionBtn'
            )
            ?.addEventListener(
                'click',
                openNewFolderModal
            );


        document
            .getElementById(
                'importBtn'
            )
            ?.addEventListener(
                'click',
                openImportModal
            );


        document
            .getElementById(
                'exportBtn'
            )
            ?.addEventListener(
                'click',
                openExportModal
            );


        document
            .getElementById(
                'paginationPrev'
            )
            ?.addEventListener(
                'click',
                () => {

                    if (
                        state.page > 1
                    ) {

                        state.page--;

                        render();

                    }

                }
            );


        document
            .getElementById(
                'paginationNext'
            )
            ?.addEventListener(
                'click',
                () => {

                    const totalPages =
                        getTotalPages();


                    if (
                        state.page <
                        totalPages
                    ) {

                        state.page++;

                        render();

                    }

                }
            );

    }


    function resetFilters() {

        state.search = '';

        state.purpose = 'all';

        state.tags.clear();

        state.segments.clear();

        state.page = 1;


        const search =
            document.getElementById(
                'collectionSearch'
            );


        if (search) {

            search.value = '';

        }


        const purpose =
            document.getElementById(
                'purposeFilter'
            );


        if (purpose) {

            purpose.value =
                'all';

        }


        applyFilters();

    }


    // ============================================================
    // TABLE EVENTS
    // ============================================================

    function wireTableEvents() {

        const tbody =
            document.getElementById(
                'folderTableBody'
            );


        if (!tbody) return;


        tbody.addEventListener(
            'contextmenu',
            event => {

                /*
                 * rmbcv.js owns the actual context menu.
                 * Keeping this listener intentionally empty prevents
                 * collectionview.js from interfering with it.
                 */

            }
        );

    }


    // ============================================================
    // FOLDER OPERATIONS
    // ============================================================

    function getFolder(
        id
    ) {

        return state.folders.find(
            folder =>
                Number(folder.id) ===
                Number(id)
        );

    }


    function duplicateFolder(
        id
    ) {

        const folder =
            getFolder(id);


        if (!folder) return;


        state.folders.push({

            ...structuredClone(
                folder
            ),

            id:
                nextId++,

            name:
                `${folder.name} (copy)`,

            source: {
                type:
                    'local-copy'
            }

        });


        renderSidebar();

        applyFilters();

    }


    function createFolder(
        name,
        purpose,
        tags,
        annotation
    ) {

        if (
            !String(name)
                .trim()
        ) {

            return;

        }


        state.folders.push({

            id:
                nextId++,

            name:
                String(name)
                    .trim(),

            purpose,

            datatype:
                'EQP Data + SQLite',

            tags:
                String(tags || '')
                    .split(',')
                    .map(
                        tag =>
                            tag.trim()
                    )
                    .filter(Boolean),

            annotation:
                String(annotation || '')
                    .trim(),

            source: {
                type:
                    'local'
            },

            endpoints: []

        });


        renderSidebar();

        applyFilters();

    }


    async function deleteFolders(ids) {

        const targets =
            ids.map(getFolder).filter(Boolean);

        const results =
            await Promise.allSettled(
                targets.map(folder => deleteFolderOnBackend(folder))
            );

        const succeededIds = [];
        const failedNames = [];

        results.forEach((result, index) => {
            const folder = targets[index];
            if (result.status === 'fulfilled' && result.value.ok) {
                succeededIds.push(Number(folder.id));
            } else {
                failedNames.push(folder.name);
            }
        });

        state.folders =
            state.folders.filter(
                folder => !succeededIds.includes(Number(folder.id))
            );

        succeededIds.forEach(id => state.selected.delete(id));

        if (succeededIds.includes(Number(state.activeFolderId))) {
            state.activeFolderId = null;
        }

        if (succeededIds.includes(Number(state.prevActiveFolderId))) {
            state.prevActiveFolderId = null;
        }

        if (failedNames.length) {
            showAlert(`Failed to delete on server: ${failedNames.join(', ')}`);
        }

        renderSidebar();
        applyFilters();
    }

    async function deleteFolderOnBackend(folder) {
    alert('About to delete: ' + folder.name);
    const response = await fetch(
        `${API_BASE}/dataview/${encodeURIComponent(folder.name)}/delete`,
        { method: 'POST' }
    );
    alert('Response status: ' + response.status);
    return response;
}


    // ============================================================
    // MERGE
    // ============================================================

    function openMergeModal(
        ids
    ) {

        const uniqueIds =
            [
                ...new Set(
                    ids.map(Number)
                )
            ];


        const sourceFolders =
            uniqueIds
                .map(getFolder)
                .filter(Boolean);


        if (
            sourceFolders.length <
            2
        ) {

            showAlert(
                'Select at least two folders to merge.'
            );

            return;

        }


        const endpointHtml =
            sourceFolders
                .map(
                    folder => {

                        const endpoints =
                            Array.isArray(
                                folder.endpoints
                            )
                                ? folder.endpoints
                                : [];


                        return `

                            <div
                                class="rounded-lg
                                       border
                                       border-slate-800
                                       bg-slate-950/50
                                       p-3"
                            >

                                <div
                                    class="mb-2 flex
                                           items-center
                                           justify-between"
                                >

                                    <div>

                                        <p
                                            class="font-medium
                                                   text-white"
                                        >
                                            ${escapeHtml(
                            folder.name
                        )}
                                        </p>

                                        <p
                                            class="text-[10px]
                                                   text-slate-500"
                                        >
                                            ${endpoints.length
                            }
                                            endpoints
                                        </p>

                                    </div>


                                    <button
                                        type="button"
                                        class="select-folder-endpoints
                                               rounded-md
                                               bg-slate-800
                                               px-2 py-1
                                               text-[10px]
                                               text-slate-400
                                               hover:bg-slate-700"
                                        data-folder-id="${folder.id}"
                                    >
                                        Select all
                                    </button>

                                </div>


                                <div
                                    class="max-h-48
                                           space-y-1
                                           overflow-y-auto"
                                >

                                    ${endpoints
                                .map(
                                    endpoint => `

                                                    <label
                                                        class="flex
                                                               items-center
                                                               gap-2
                                                               rounded
                                                               px-2 py-1.5
                                                               hover:bg-slate-800"
                                                    >

                                                        <input
                                                            type="checkbox"
                                                            class="merge-endpoint
                                                                   h-3.5 w-3.5
                                                                   accent-cyan-400"
                                                            data-folder-id="${folder.id}"
                                                            data-endpoint-id="${escapeAttr(
                                        endpoint.id
                                    )}"
                                                        >

                                                        <span
                                                            class="text-[10px]
                                                                   font-semibold
                                                                   ${methodClass(
                                        endpoint.method
                                    )}"
                                                        >
                                                            ${escapeHtml(
                                        endpoint.method
                                    )}
                                                        </span>

                                                        <span
                                                            class="truncate
                                                                   font-mono
                                                                   text-[10px]
                                                                   text-slate-400"
                                                        >
                                                            ${escapeHtml(
                                        endpoint.endpoint
                                    )}
                                                        </span>

                                                    </label>

                                                `
                                )
                                .join('')
                            }

                                </div>

                            </div>

                        `;

                    }
                )
                .join('');


        openModal(`

            <div
                class="flex items-start
                       justify-between
                       border-b border-slate-800
                       pb-3"
            >

                <div>

                    <h2
                        class="text-sm font-semibold
                               text-white"
                    >
                        Merge Collections
                    </h2>

                    <p
                        class="mt-0.5 text-[11px]
                               text-slate-500"
                    >
                        Choose endpoints from
                        the selected collections.
                    </p>

                </div>


                <button
                    type="button"
                    data-modal-close
                    class="rounded-md p-1
                           text-slate-500
                           hover:bg-slate-800
                           hover:text-white"
                >
                    ${iconClose()}
                </button>

            </div>


            <div class="space-y-3 py-4">

                ${endpointHtml}

            </div>


            <div
                class="grid grid-cols-1
                       gap-3 sm:grid-cols-2"
            >

                <div>

                    <label
                        class="mb-1 block
                               text-[11px]
                               text-slate-500"
                    >
                        New folder name
                    </label>

                    <input
                        id="mergeName"
                        class="h-9 w-full rounded-md
                               border border-slate-700
                               bg-slate-950 px-3
                               text-xs outline-none
                               focus:border-cyan-500"
                        placeholder="Merged collection"
                    >

                </div>


                <div>

                    <label
                        class="mb-1 block
                               text-[11px]
                               text-slate-500"
                    >
                        Tags
                    </label>

                    <input
                        id="mergeTags"
                        class="h-9 w-full rounded-md
                               border border-slate-700
                               bg-slate-950 px-3
                               text-xs outline-none
                               focus:border-cyan-500"
                        placeholder="merged, production"
                    >

                </div>

            </div>


            <label
                class="mb-1 mt-3 block
                       text-[11px] text-slate-500"
            >
                Annotation
            </label>


            <textarea
                id="mergeAnnotation"
                class="h-20 w-full resize-none
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3 py-2
                       text-xs outline-none
                       focus:border-cyan-500"
                placeholder="Describe the merged collection..."
            ></textarea>


            <div
                class="mt-4 flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5 text-xs
                           text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="confirmMerge"
                    type="button"
                    class="rounded-md
                           bg-cyan-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-cyan-500"
                >
                    Create Merge
                </button>

            </div>

        `);


        document
            .querySelectorAll(
                '.select-folder-endpoints'
            )
            .forEach(
                button => {

                    button.addEventListener(
                        'click',
                        () => {

                            const folderId =
                                button.dataset
                                    .folderId;


                            document
                                .querySelectorAll(
                                    `.merge-endpoint[data-folder-id="${folderId}"]`
                                )
                                .forEach(
                                    checkbox =>
                                        checkbox.checked =
                                        true
                                );

                        }
                    );

                }
            );


        document
            .getElementById(
                'confirmMerge'
            )
            ?.addEventListener(
                'click',
                () => {

                    const selectedEndpoints =
                        [
                            ...document.querySelectorAll(
                                '.merge-endpoint:checked'
                            )
                        ];


                    if (
                        !selectedEndpoints.length
                    ) {

                        showAlert(
                            'Select at least one endpoint.'
                        );

                        return;

                    }


                    const mergedEndpoints =
                        [];


                    selectedEndpoints.forEach(
                        checkbox => {

                            const folder =
                                getFolder(
                                    Number(
                                        checkbox.dataset
                                            .folderId
                                    )
                                );


                            const endpoint =
                                folder?.endpoints?.find(
                                    ep =>
                                        String(
                                            ep.id
                                        ) ===
                                        String(
                                            checkbox.dataset
                                                .endpointId
                                        )
                                );


                            if (
                                endpoint
                            ) {

                                mergedEndpoints.push({

                                    ...structuredClone(
                                        endpoint
                                    ),

                                    sourceFolderId:
                                        folder.id,

                                    sourceFolderName:
                                        folder.name

                                });

                            }

                        }
                    );


                    const name =
                        document
                            .getElementById(
                                'mergeName'
                            )
                            .value
                            .trim();


                    const tags =
                        document
                            .getElementById(
                                'mergeTags'
                            )
                            .value
                            .split(',')
                            .map(
                                tag =>
                                    tag.trim()
                            )
                            .filter(Boolean);


                    state.folders.push({

                        id:
                            nextId++,

                        name:
                            name ||
                            'Merged Collection',

                        purpose:
                            'importable',

                        datatype:
                            'EQP Data + SQLite',

                        tags,

                        annotation:
                            document
                                .getElementById(
                                    'mergeAnnotation'
                                )
                                .value
                                .trim(),

                        source: {
                            type:
                                'merged',

                            sourceFolderIds:
                                uniqueIds

                        },

                        endpoints:
                            mergedEndpoints

                    });


                    renderSidebar();

                    closeModal();

                    applyFilters();

                }
            );

    }


    // ============================================================
    // DATA VIEW
    // ============================================================

    function openDataView(
        id
    ) {

        window.open(
            `./collection_dataview.html?folder=${encodeURIComponent(id)}`,
            '_blank'
        );

    }


    // ============================================================
    // NEW FOLDER
    // ============================================================

    function openNewFolderModal() {

        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                New Empty Folder
            </h2>


            <label
                class="mb-1 block
                       text-[11px] text-slate-500"
            >
                Folder name
            </label>

            <input
                id="newFolderName"
                class="mb-3 h-9 w-full
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3
                       text-xs outline-none
                       focus:border-cyan-500"
                placeholder="My New Collection"
            >


            <label
                class="mb-1 block
                       text-[11px] text-slate-500"
            >
                Purpose
            </label>

            <select
                id="newFolderPurpose"
                class="mb-3 h-9 w-full
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3
                       text-xs outline-none"
            >
                <option value="importable">
                    Importable
                </option>

                <option value="exportable">
                    Exportable
                </option>
            </select>


            <label
                class="mb-1 block
                       text-[11px] text-slate-500"
            >
                Tags
            </label>

            <input
                id="newFolderTags"
                class="mb-3 h-9 w-full
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3
                       text-xs outline-none
                       focus:border-cyan-500"
                placeholder="personal, draft"
            >


            <label
                class="mb-1 block
                       text-[11px] text-slate-500"
            >
                Annotation
            </label>

            <textarea
                id="newFolderAnnotation"
                class="mb-4 h-20 w-full
                       resize-none rounded-md
                       border border-slate-700
                       bg-slate-950 px-3 py-2
                       text-xs outline-none
                       focus:border-cyan-500"
            ></textarea>


            <div
                class="flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="confirmNewFolder"
                    type="button"
                    class="rounded-md
                           bg-cyan-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-cyan-500"
                >
                    Create
                </button>

            </div>

        `);


        document
            .getElementById(
                'confirmNewFolder'
            )
            ?.addEventListener(
                'click',
                () => {

                    createFolder(

                        document
                            .getElementById(
                                'newFolderName'
                            )
                            .value,

                        document
                            .getElementById(
                                'newFolderPurpose'
                            )
                            .value,

                        document
                            .getElementById(
                                'newFolderTags'
                            )
                            .value,

                        document
                            .getElementById(
                                'newFolderAnnotation'
                            )
                            .value

                    );


                    closeModal();

                }
            );

    }


    // ============================================================
    // RENAME
    // ============================================================

    function openRenameModal(
        id
    ) {

        const folder =
            getFolder(id);


        if (!folder) return;


        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                Rename Collection
            </h2>


            <input
                id="renameInput"
                value="${escapeAttr(
            folder.name
        )}"
                class="mb-4 h-9 w-full
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3
                       text-xs outline-none
                       focus:border-cyan-500"
            >


            <div
                class="flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="confirmRename"
                    type="button"
                    class="rounded-md
                           bg-cyan-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-cyan-500"
                >
                    Save
                </button>

            </div>

        `);


        document
            .getElementById(
                'confirmRename'
            )
            ?.addEventListener(
                'click',
                () => {

                    const name =
                        document
                            .getElementById(
                                'renameInput'
                            )
                            .value
                            .trim();


                    if (!name) {

                        showAlert(
                            'Collection name cannot be empty.'
                        );

                        return;

                    }


                    folder.name =
                        name;


                    closeModal();

                    applyFilters();

                }
            );

    }


    // ============================================================
    // TAG EDITOR
    // ============================================================

    function openTagEditor(
        id
    ) {

        const folder =
            getFolder(id);


        if (!folder) return;


        const currentTags =
            Array.isArray(
                folder.tags
            )
                ? folder.tags.join(', ')
                : '';


        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                Edit Tags
            </h2>


            <input
                id="collectionTagInput"
                value="${escapeAttr(
            currentTags
        )}"
                class="h-9 w-full
                       rounded-md
                       border border-slate-700
                       bg-slate-950 px-3
                       text-xs outline-none
                       focus:border-cyan-500"
            >


            <p
                class="mt-1 text-[10px]
                       text-slate-600"
            >
                Separate tags with commas.
            </p>


            <div
                class="mt-4 flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="saveCollectionTags"
                    type="button"
                    class="rounded-md
                           bg-cyan-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-cyan-500"
                >
                    Save
                </button>

            </div>

        `);


        document
            .getElementById(
                'saveCollectionTags'
            )
            ?.addEventListener(
                'click',
                () => {

                    folder.tags =
                        document
                            .getElementById(
                                'collectionTagInput'
                            )
                            .value
                            .split(',')
                            .map(
                                tag =>
                                    tag.trim()
                            )
                            .filter(Boolean);


                    closeModal();

                    renderSidebar();

                    applyFilters();

                }
            );

    }


    // ============================================================
    // ANNOTATION EDITOR
    // ============================================================

    function openAnnotationEditor(
        id
    ) {

        const folder =
            getFolder(id);


        if (!folder) return;


        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                Edit Annotation
            </h2>


            <textarea
                id="collectionAnnotationInput"
                class="h-28 w-full resize-y
                       rounded-md
                       border border-slate-700
                       bg-slate-950 p-3
                       text-xs leading-5
                       text-slate-300
                       outline-none
                       focus:border-cyan-500"
            >${escapeHtml(
            folder.annotation || ''
        )}</textarea>


            <div
                class="mt-4 flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="saveCollectionAnnotation"
                    type="button"
                    class="rounded-md
                           bg-cyan-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-cyan-500"
                >
                    Save
                </button>

            </div>

        `);


        document
            .getElementById(
                'saveCollectionAnnotation'
            )
            ?.addEventListener(
                'click',
                () => {

                    folder.annotation =
                        document
                            .getElementById(
                                'collectionAnnotationInput'
                            )
                            .value
                            .trim();


                    closeModal();

                    render();

                }
            );

    }


    // ============================================================
    // DELETE
    // ============================================================

    function openDeleteModal(
        ids
    ) {

        const folders =
            ids
                .map(getFolder)
                .filter(Boolean);


        if (!folders.length) return;


        openModal(`

            <div
                class="flex items-start
                       gap-3 rounded-lg
                       border
                       border-rose-500/20
                       bg-rose-500/5 p-3"
            >

                <div
                    class="mt-0.5
                           text-rose-400"
                >
                    ${iconTrash()}
                </div>


                <div>

                    <h2
                        class="text-sm
                               font-semibold
                               text-rose-300"
                    >
                        Delete
                        ${folders.length > 1
                ? 'Collections'
                : 'Collection'
            }
                    </h2>

                    <p
                        class="mt-1 text-[11px]
                               text-slate-500"
                    >
                        This removes the selected
                        collection(s) from the
                        current prototype session.
                    </p>

                </div>

            </div>


            <div
                class="my-4 max-h-48
                       overflow-y-auto
                       rounded-lg
                       border border-slate-800"
            >

                ${folders
                .map(
                    folder => `

                                <div
                                    class="border-b
                                           border-slate-800
                                           px-3 py-2
                                           last:border-0"
                                >

                                    <p
                                        class="font-mono
                                               text-xs
                                               text-slate-300"
                                    >
                                        ${escapeHtml(
                        folder.name
                    )}
                                    </p>

                                    <p
                                        class="mt-0.5
                                               text-[10px]
                                               text-slate-600"
                                    >
                                        ${Array.isArray(
                        folder.endpoints
                    )
                            ? folder.endpoints.length
                            : 0
                        }
                                        endpoints
                                    </p>

                                </div>

                            `
                )
                .join('')
            }

            </div>


            <div
                class="flex justify-end
                       gap-2"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Cancel
                </button>


                <button
                    id="confirmDelete"
                    type="button"
                    class="rounded-md
                           bg-rose-600
                           px-3 py-1.5
                           text-xs font-medium
                           text-white
                           hover:bg-rose-500"
                >
                    Delete
                </button>

            </div>

        `);


        document.getElementById('confirmDelete')?.addEventListener('click', async event => {
            const button = event.currentTarget;
            button.disabled = true;
            button.textContent = 'Deleting…';
            await deleteFolders(ids);
            closeModal();
        });

    }


    // ============================================================
    // IMPORT / EXPORT
    // ============================================================

    function openImportModal() {

        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                Import Collection
            </h2>


            <p
                class="text-xs leading-5
                       text-slate-400"
            >
                The backend will eventually process
                an exported EDMS package.
                For this prototype, the shared
                <code class="text-cyan-400">
                    collections.json
                </code>
                represents that package.
            </p>


            <div
                class="mt-4 rounded-lg
                       border border-slate-800
                       bg-slate-950/60 p-3
                       text-xs text-slate-400"
            >
                ${state.folders.length
            }
                shared folders are currently
                loaded as prototype data.
            </div>


            <div
                class="mt-4 flex justify-end"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Close
                </button>

            </div>

        `);

    }


    function openExportModal() {

        const selected =
            getSelectedFolders();


        const active =
            getFolder(
                state.activeFolderId
            );


        const targets =
            selected.length
                ? selected
                : active
                    ? [active]
                    : [];


        openModal(`

            <h2
                class="mb-4 text-sm font-semibold
                       text-white"
            >
                Export Collection
            </h2>


            <p
                class="text-xs leading-5
                       text-slate-400"
            >
                Exported data uses the same
                EDMS collection-package structure
                used by
                <code class="text-cyan-400">
                    collections.json
                </code>.
            </p>


            <div
                class="my-4 rounded-lg
                       border border-slate-800
                       bg-slate-950/60 p-3"
            >

                ${targets.length

                ? targets
                    .map(
                        folder => `

                                    <div
                                        class="flex
                                               items-center
                                               justify-between
                                               border-b
                                               border-slate-800
                                               py-2 last:border-0"
                                    >

                                        <span
                                            class="truncate
                                                   text-xs
                                                   text-slate-300"
                                        >
                                            ${escapeHtml(
                            folder.name
                        )}
                                        </span>

                                        <span
                                            class="ml-3
                                                   shrink-0
                                                   text-[10px]
                                                   text-slate-600"
                                        >
                                            ${Array.isArray(
                            folder.endpoints
                        )
                                ? folder.endpoints.length
                                : 0
                            }
                                            endpoints
                                        </span>

                                    </div>

                                `
                    )
                    .join('')

                : `
                            <span
                                class="text-xs
                                       text-slate-600"
                            >
                                Select a collection first.
                            </span>
                          `
            }

            </div>


            <div
                class="flex justify-end"
            >

                <button
                    type="button"
                    data-modal-close
                    class="rounded-md
                           border border-slate-700
                           px-3 py-1.5
                           text-xs text-slate-400
                           hover:bg-slate-800"
                >
                    Close
                </button>

            </div>

        `);

    }


    // ============================================================
    // MODAL
    // ============================================================

    function wireModal() {

        const backdrop =
            document.getElementById(
                'modalBackdrop'
            );


        if (!backdrop) return;


        backdrop.addEventListener(
            'click',
            event => {

                if (
                    event.target ===
                    backdrop
                ) {

                    closeModal();

                }


                if (
                    event.target.closest(
                        '[data-modal-close]'
                    )
                ) {

                    closeModal();

                }

            }
        );


        document.addEventListener(
            'keydown',
            event => {

                if (
                    event.key ===
                    'Escape' &&
                    !backdrop.classList.contains(
                        'hidden'
                    )
                ) {

                    closeModal();

                }

            }
        );

    }


    function openModal(
        html
    ) {

        const backdrop =
            document.getElementById(
                'modalBackdrop'
            );


        const content =
            document.getElementById(
                'modalContent'
            );


        if (
            !backdrop ||
            !content
        ) {

            return;

        }


        content.innerHTML =
            html;


        backdrop.classList.remove(
            'hidden'
        );


        backdrop.classList.add(
            'flex'
        );

    }


    function closeModal() {

        const backdrop =
            document.getElementById(
                'modalBackdrop'
            );


        const content =
            document.getElementById(
                'modalContent'
            );


        backdrop?.classList.add(
            'hidden'
        );


        backdrop?.classList.remove(
            'flex'
        );


        if (content) {

            content.innerHTML =
                '';

        }

    }


    // ============================================================
    // UTILITIES
    // ============================================================

    function calculateSize(
        folder
    ) {

        const endpoints =
            Array.isArray(
                folder.endpoints
            )
                ? folder.endpoints
                : [];


        const tags =
            Array.isArray(
                folder.tags
            )
                ? folder.tags
                : [];


        return Math.max(
            0.1,
            endpoints.length * 1.7 +
            tags.length * 0.6
        );

    }


    function capitalize(
        value
    ) {

        const text =
            String(
                value || ''
            );


        return text
            ? text.charAt(0).toUpperCase() +
            text.slice(1)
            : '';

    }


    function methodClass(
        method
    ) {

        return {

            GET:
                'text-emerald-400',

            POST:
                'text-sky-400',

            PUT:
                'text-amber-400',

            PATCH:
                'text-violet-400',

            DELETE:
                'text-rose-400'

        }[
            String(
                method || ''
            ).toUpperCase()
        ] || 'text-white';

    }


    function escapeHtml(
        value
    ) {

        const div =
            document.createElement(
                'div'
            );


        div.textContent =
            String(
                value ?? ''
            );


        return div.innerHTML;

    }


    function escapeAttr(
        value
    ) {

        return escapeHtml(
            value
        )
            .replace(
                /"/g,
                '&quot;'
            );

    }


    function showAlert(
        message
    ) {

        /*
         * Kept deliberately simple for now.
         * This avoids changing the project's existing
         * notification architecture.
         */

        window.alert(
            message
        );

    }


    // ============================================================
    // ICONS
    // ============================================================

    function iconClose() {

        return `

            <svg
                class="h-4 w-4"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
            >
                <path d="M6 6l12 12"/>
                <path d="M18 6 6 18"/>
            </svg>

        `;

    }


    function iconTrash() {

        return `

            <svg
                class="h-4 w-4"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
            >
                <path d="M4 7h16"/>
                <path d="M10 11v6M14 11v6"/>
                <path d="M6 7l1 13h10l1-13"/>
                <path d="M9 7V4h6v3"/>
            </svg>

        `;

    }


    // ============================================================
    // PUBLIC API
    // ============================================================

    window.CollectionView = {

        getState:
            () =>
                state,

        getFolder,

        setActiveFolder,

        duplicateFolder,

        openRenameModal,

        openTagEditor,

        openAnnotationEditor,

        openDeleteModal,

        openMergeModal,

        openNewFolderModal,

        openDataView,

        applyFilters,

        resetFilters

    };

})();