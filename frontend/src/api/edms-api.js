// ============================================================
// EDMS API
// BACKEND COMMUNICATION LAYER
// ============================================================

(() => {
    'use strict';


    // ============================================================
    // CONFIG
    // ============================================================

    const API_BASE = 'http://localhost:3000';
    const WS_BASE = 'ws://localhost:3000';


    // ============================================================
    // HTTP HELPER
    // ============================================================

    async function http(
        method,
        path,
        body
    ) {

        const options = {
            method,

            headers: {
                'Content-Type': 'application/json'
            }
        };


        if (body !== undefined) {

            options.body =
                JSON.stringify(body);

        }


        const response =
            await fetch(
                `${API_BASE}${path}`,
                options
            );


        const text =
            await response.text();


        let data;


        try {

            data =
                text
                    ? JSON.parse(text)
                    : null;

        } catch {

            data =
                text;

        }


        return {

            status:
                response.status,

            ok:
                response.ok,

            data

        };

    }


    // ============================================================
    // WEBSOCKET HELPER
    // ============================================================

    function createWebSocket(
        path,
        options = {}
    ) {

        return new WebSocket(
            `${WS_BASE}${path}`,
            options.protocols
        );

    }


    // ============================================================
    // R1 — ENDPOINTS
    // ============================================================

    async function registerEndpoint(
        endpointId,
        endpointUrl,
        method,
        annotation
    ) {

        const body = {

            endpoint_id:
                endpointId,

            endpoint_str:
                endpointUrl

        };


        if (method !== undefined) {

            body.method =
                method;

        }


        if (annotation !== undefined) {

            body.annotation =
                annotation;

        }


        return http(
            'POST',
            '/endpoints/create',
            body
        );

    }


    // ============================================================
    // ONE-TIME BULK REGISTRATION UTILITY
    // ============================================================

    async function registerAllEndpoints(
        endpointList
    ) {

        const results = [];


        for (
            const endpoint of endpointList
        ) {

            const url =
                (endpoint.baseUrl || '') +
                (endpoint.endpoint || '');


            const result =
                await registerEndpoint(
                    endpoint.id,
                    url,
                    endpoint.method,
                    endpoint.annotation
                );


            results.push({

                id:
                    endpoint.id,

                status:
                    result.status,

                ok:
                    result.ok

            });


            console.log(
                `Registered ${endpoint.id}:`,
                result.status
            );

        }


        return results;

    }


    // ============================================================
    // DELETE ENDPOINT
    // ============================================================

    async function deleteEndpoint(
        endpointId
    ) {

        return http(
            'POST',
            `/endpoints/${encodeURIComponent(endpointId)}/delete`
        );

    }


    // ============================================================
    // UPDATE ENDPOINT ANNOTATION
    // ============================================================

    async function setEndpointAnnotation(
        endpointId,
        annotation
    ) {

        return http(
            'POST',
            `/endpoints/${encodeURIComponent(endpointId)}/annotation`,
            {
                annotation
            }
        );

    }


    // ============================================================
    // STATIC VIEW METADATA
    // ============================================================

    async function getHome() {

        return http(
            'GET',
            '/home'
        );

    }


    async function getTestView() {

        return http(
            'GET',
            '/test-view'
        );

    }


    async function getListView() {

        return http(
            'GET',
            '/list-view'
        );

    }


    // ============================================================
    // TEST VIEW — WEBSOCKETS
    // ============================================================

    function connectTestView() {

        return createWebSocket(
            '/test-view/run'
        );

    }


    function connectEndpointLoader() {

        return createWebSocket(
            '/test-view/endpoints/load'
        );

    }


    function connectBookmarkLoader(collection) {

        return createWebSocket(
            `/test-view/${encodeURIComponent(collection)}/bookmarks/load`
        );

    }


    function connectHistoryLoader() {

        return createWebSocket(
            '/test-view/history/load'
        );

    }


    // ============================================================
    // TEST VIEW — START TEST
    // ============================================================

    function startTest(
        ws,
        endpointId,
        endpointStr,
        method,
        body = {},
        timeoutMs = 30000,
        tickIntervalMs = 500,
        headers,
        annotation
    ) {

        const payload = {

            endpoint_str:
                endpointStr,

            method:
                method,

            body:
                body,

            timeout_ms:
                timeoutMs,

            tick_interval_ms:
                tickIntervalMs

        };


        // endpoint_id became optional in the
        // backend update from 2026-09-08.

        if (
            endpointId !== undefined &&
            endpointId !== null &&
            endpointId !== ''
        ) {

            payload.endpoint_id =
                endpointId;

        }


        if (
            headers !== undefined
        ) {

            payload.headers =
                headers;

        }


        if (
            annotation !== undefined
        ) {

            payload.annotation =
                annotation;

        }


        ws.send(
            JSON.stringify({

                type:
                    'run_test',

                payload

            })
        );

    }


    // ============================================================
    // TEST VIEW — WAIT FOR TEST EVENTS
    // ============================================================

    function waitForTestFinished(
        ws,
        handlers = {}
    ) {

        return new Promise(
            (
                resolve,
                reject
            ) => {

                function handleMessage(
                    wsEvent
                ) {

                    try {

                        const message =
                            JSON.parse(
                                wsEvent.data
                            );


                        const event =
                            message.event;


                        if (!event) {

                            return;

                        }


                        // ----------------------------------------
                        // Test Started
                        // ----------------------------------------

                        if (
                            event.type ===
                            'TestStarted'
                        ) {

                            if (
                                typeof handlers.onStarted ===
                                'function'
                            ) {

                                handlers.onStarted(
                                    event
                                );

                            }

                            return;

                        }


                        // ----------------------------------------
                        // Timer Tick
                        // ----------------------------------------

                        if (
                            event.type ===
                            'TimerTick'
                        ) {

                            if (
                                typeof handlers.onTick ===
                                'function'
                            ) {

                                handlers.onTick(
                                    event
                                );

                            }

                            return;

                        }


                        // ----------------------------------------
                        // Test Finished
                        // ----------------------------------------

                        if (
                            event.type ===
                            'TestFinished'
                        ) {

                            cleanup();


                            if (
                                typeof handlers.onFinished ===
                                'function'
                            ) {

                                handlers.onFinished(
                                    event
                                );

                            }


                            resolve(
                                event
                            );


                            return;

                        }


                        // ----------------------------------------
                        // Test Timeout
                        // ----------------------------------------

                        if (
                            event.type ===
                            'TestTimeout'
                        ) {

                            cleanup();


                            if (
                                typeof handlers.onTimeout ===
                                'function'
                            ) {

                                handlers.onTimeout(
                                    event
                                );

                            }


                            reject(
                                new Error(
                                    'Backend reported TestTimeout'
                                )
                            );


                            return;

                        }


                        // ----------------------------------------
                        // Backend Error
                        // ----------------------------------------

                        if (
                            event.type ===
                            'Error'
                        ) {

                            cleanup();


                            if (
                                typeof handlers.onError ===
                                'function'
                            ) {

                                handlers.onError(
                                    event
                                );

                            }


                            reject(
                                new Error(
                                    event.payload?.message ||
                                    'Backend returned Error'
                                )
                            );


                            return;

                        }

                    } catch (error) {

                        cleanup();

                        reject(
                            error
                        );

                    }

                }


                function handleError(
                    error
                ) {

                    cleanup();

                    reject(
                        error
                    );

                }


                function cleanup() {

                    ws.removeEventListener(
                        'message',
                        handleMessage
                    );


                    ws.removeEventListener(
                        'error',
                        handleError
                    );

                }


                ws.addEventListener(
                    'message',
                    handleMessage
                );


                ws.addEventListener(
                    'error',
                    handleError
                );

            }
        );

    }


    // ============================================================
    // TEST VIEW — REQUEST
    // ============================================================

    async function fetchRequest(
        endpointId,
        requestNumber
    ) {

        return http(
            'GET',
            `/test-view/` +
            `${encodeURIComponent(endpointId)}` +
            `/request/` +
            `${encodeURIComponent(requestNumber)}`
        );

    }


    // ============================================================
    // TEST VIEW — RESPONSE
    // ============================================================

    async function fetchResponse(
        endpointId,
        requestNumber
    ) {

        return http(
            'GET',
            `/test-view/` +
            `${encodeURIComponent(endpointId)}` +
            `/response/` +
            `${encodeURIComponent(requestNumber)}`
        );

    }


    // ============================================================
    // TEST VIEW — HEADERS
    // ============================================================

    async function fetchHeaders(
        endpointId,
        requestNumber
    ) {

        return http(
            'GET',
            `/test-view/` +
            `${encodeURIComponent(endpointId)}` +
            `/headers/` +
            `${encodeURIComponent(requestNumber)}`
        );

    }


    // ============================================================
    // TEST VIEW — STOP TEST
    // ============================================================

    async function stopTest(
        endpointId,
        requestNumber
    ) {

        return http(
            'POST',
            '/test-view/stop',
            {
                endpoint_id:
                    endpointId,

                request_number:
                    requestNumber
            }
        );

    }


    // ============================================================
    // TEST VIEW — HISTORY
    // ============================================================

    async function saveHistory(
        endpointId,
        action,
        details
    ) {

        return http(
            'POST',
            '/test-view/save/history',
            {
                endpoint_id:
                    endpointId,

                action,

                details:
                    details === undefined ||
                    details === null
                        ? undefined
                        : typeof details === 'string'
                            ? details
                            : JSON.stringify(details)
            }
        );

    }


    async function clearHistory() {

        return http(
            'POST',
            '/test-view/history/clearall'
        );

    }


    // ============================================================
    // TEST VIEW — LEGACY BOOKMARK
    // ============================================================

    async function saveBookmark(
        endpointId,
        notes,
        collection
    ) {

        return http(
            'POST',
            '/test-view/save/bookmark',
            {
                endpoint_id:
                    endpointId,

                notes,

                collection
            }
        );

    }


    async function clearBookmarks(collection) {

        return http(
            'POST',
            `/test-view/${encodeURIComponent(collection)}/bookmark/clearall`
        );

    }


    // ============================================================
    // TEST VIEW — ACTIVE BOOKMARK
    // ============================================================

    function addActiveBookmark(
        collection,
        endpointId
    ) {

        return new Promise(
            (
                resolve,
                reject
            ) => {

                const ws =
                    createWebSocket(
                        `/test-view/${encodeURIComponent(collection)}/add`
                    );


                let settled =
                    false;


                const finish =
                    (
                        callback,
                        value
                    ) => {

                        if (settled) {

                            return;

                        }


                        settled =
                            true;


                        try {

                            ws.close();

                        } catch {}


                        callback(
                            value
                        );

                    };


                ws.addEventListener(
                    'open',
                    () => {

                        ws.send(
                            JSON.stringify({

                                endpoint_id:
                                    endpointId

                            })
                        );

                    }
                );


                ws.addEventListener(
                    'message',
                    event => {

                        try {

                            const message =
                                JSON.parse(
                                    event.data
                                );


                            if (
                                message.type ===
                                'error'
                            ) {

                                finish(
                                    reject,
                                    new Error(
                                        message.message ||
                                        message.payload?.message ||
                                        'Could not add endpoint to bookmarks.'
                                    )
                                );


                                return;

                            }


                            finish(
                                resolve,
                                message
                            );

                        } catch (error) {

                            finish(
                                reject,
                                error
                            );

                        }

                    }
                );


                ws.addEventListener(
                    'error',
                    error => {

                        finish(
                            reject,
                            error
                        );

                    }
                );

            }
        );

    }


    async function saveActiveBookmark(
        collection,
        endpointId
    ) {

        return http(
            'POST',
            `/bookmarks/` +
            `${encodeURIComponent(collection)}/` +
            `${encodeURIComponent(endpointId)}/save`
        );

    }


    async function unsaveActiveBookmark(
        collection,
        endpointId
    ) {

        return http(
            'POST',
            `/bookmarks/` +
            `${encodeURIComponent(collection)}/` +
            `${encodeURIComponent(endpointId)}/unsave`
        );

    }


    // ============================================================
    // COLLECTIONS
    // ============================================================

    async function listCollections() {

        return http(
            'GET',
            '/collections/list'
        );

    }


    async function getCollection(
        name
    ) {

        return http(
            'GET',
            `/collections/${encodeURIComponent(name)}`
        );

    }


    async function createCollection(
        name
    ) {

        return http(
            'POST',
            '/collections/create',
            {
                name
            }
        );

    }


    async function renameCollection(
        name,
        newName
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(name)}/rename`,
            {
                new_name:
                    newName
            }
        );

    }


    async function deleteCollection(
        name
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(name)}/delete`
        );

    }

    async function setCollectionAnnotation(
        name,
        annotation
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(name)}/annotation`,
            {
                annotation
            }
        );

    }


    async function listCollectionEndpoints(
        name
    ) {

        return http(
            'GET',
            `/collections/${encodeURIComponent(name)}/endpoints`
        );

    }


    async function removeEndpointFromCollection(
        name,
        endpointId
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(name)}/endpoints/remove`,
            {
                endpoint_id:
                    endpointId
            }
        );

    }


    // ============================================================
    // COLLECTIONS — LOAD INTO ACTIVE BOOKMARK WORKSPACE
    // ============================================================

    function loadCollection(
        name
    ) {

        return new Promise(
            (
                resolve,
                reject
            ) => {

                const ws =
                    createWebSocket(
                        `/bookmarks/` +
                        `${encodeURIComponent(name)}/load`
                    );


                let settled =
                    false;


                const finish =
                    (
                        callback,
                        value
                    ) => {

                        if (settled) {

                            return;

                        }


                        settled =
                            true;


                        try {

                            ws.close();

                        } catch {}


                        callback(
                            value
                        );

                    };


                ws.addEventListener(
                    'message',
                    event => {

                        try {

                            const message =
                                JSON.parse(
                                    event.data
                                );


                            if (
                                message.type ===
                                'error'
                            ) {

                                finish(
                                    reject,
                                    new Error(
                                        message.message ||
                                        message.payload?.message ||
                                        'Could not load collection.'
                                    )
                                );


                                return;

                            }


                            finish(
                                resolve,
                                message
                            );

                        } catch (error) {

                            finish(
                                reject,
                                error
                            );

                        }

                    }
                );


                ws.addEventListener(
                    'error',
                    error => {

                        finish(
                            reject,
                            error
                        );

                    }
                );

            }
        );

    }


    // ============================================================
    // COLLECTION TAG ROLLUPS — GLOBAL TAGS
    // ============================================================

    async function createGlobalTag(
        name,
        endpointIds
    ) {

        return http(
            'POST',
            '/collections/tags/create',
            {
                name,

                endpoint_ids:
                    endpointIds
            }
        );

    }


    async function deleteGlobalTags(
        names
    ) {

        return http(
            'POST',
            '/collections/tags/delete',
            {
                names
            }
        );

    }


    async function renameGlobalTag(
        oldName,
        newName
    ) {

        return http(
            'POST',
            '/collections/tags/rename',
            {
                old_name:
                    oldName,

                new_name:
                    newName
            }
        );

    }


    async function listGlobalTags() {

        return http(
            'GET',
            '/collections/tags/list'
        );

    }


    // ============================================================
    // COLLECTION — MEMBERSHIP TAGS
    // ============================================================

    async function addMembershipTag(
        collectionName,
        tag
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(collectionName)}` +
            `/membership-tags/add`,
            {
                tag
            }
        );

    }


    async function removeMembershipTag(
        collectionName,
        tag
    ) {

        return http(
            'POST',
            `/collections/${encodeURIComponent(collectionName)}` +
            `/membership-tags/remove`,
            {
                tag
            }
        );

    }


    async function listMembershipTags(
        collectionName
    ) {

        return http(
            'GET',
            `/collections/${encodeURIComponent(collectionName)}` +
            `/membership-tags`
        );

    }


    async function listCollectionsByTag(
        tagName
    ) {

        return http(
            'GET',
            `/collections/by-tag/${encodeURIComponent(tagName)}`
        );

    }


    // ============================================================
    // PER-ENDPOINT TAGS
    // ============================================================

    async function listPopularTags() {

        return http(
            'GET',
            '/tags/popular'
        );

    }
    async function deleteActiveBookmark(endpointId) {

    return new Promise((resolve, reject) => {

        const ws =
            createWebSocket(
                "/test-view/active/delete"
            );

        let settled = false;

        const finish = (
            callback,
            value
        ) => {

            if (settled) return;

            settled = true;

            try {
                ws.close();
            } catch {}

            callback(value);
        };

        ws.addEventListener(
            "open",
            () => {

                ws.send(
                    JSON.stringify({
                        endpoint_id:
                            endpointId
                    })
                );

            }
        );

        ws.addEventListener(
            "message",
            event => {

                try {

                    const message =
                        JSON.parse(
                            event.data
                        );

                    if (
                        message.type === "error"
                    ) {

                        finish(
                            reject,
                            new Error(
                                message.message ||
                                "Bookmark deletion failed."
                            )
                        );

                        return;
                    }

                    finish(
                        resolve,
                        message
                    );

                } catch (error) {

                    finish(
                        reject,
                        error
                    );

                }

            }
        );

        ws.addEventListener(
            "error",
            () => {

                finish(
                    reject,
                    new Error(
                        "Bookmark delete WebSocket failed."
                    )
                );

            }
        );

    });
}


    async function listEndpointTags(
        endpointId
    ) {

        return http(
            'GET',
            `/tags/${encodeURIComponent(endpointId)}`
        );

    }


    async function addEndpointTag(
        endpointId,
        tag
    ) {

        return http(
            'POST',
            `/tags/${encodeURIComponent(endpointId)}/add`,
            {
                tag
            }
        );

    }


    async function removeEndpointTag(
        endpointId,
        tag
    ) {

        return http(
            'POST',
            `/tags/${encodeURIComponent(endpointId)}/remove`,
            {
                tag
            }
        );

    }


    // ============================================================
    // DATA VIEW
    // ============================================================

    async function deleteDataViewFolder(
        folder
    ) {

        return http(
            'POST',
            `/dataview/${encodeURIComponent(folder)}/delete`
        );

    }


    async function mergeDataViewFolder(
        folder
    ) {

        return http(
            'POST',
            `/dataview/${encodeURIComponent(folder)}/merge`
        );

    }


    function activateDataViewFolder(
        folder
    ) {

        return new Promise(
            (
                resolve,
                reject
            ) => {

                const ws =
                    createWebSocket(
                        `/dataview/${encodeURIComponent(folder)}/active`
                    );


                let settled =
                    false;


                const finish =
                    (
                        callback,
                        value
                    ) => {

                        if (settled) {

                            return;

                        }


                        settled =
                            true;


                        callback(
                            value
                        );

                    };


                ws.addEventListener(
                    'message',
                    event => {

                        try {

                            const message =
                                JSON.parse(
                                    event.data
                                );


                            finish(
                                resolve,
                                message
                            );

                        } catch (error) {

                            finish(
                                reject,
                                error
                            );

                        }

                    }
                );


                ws.addEventListener(
                    'error',
                    error => {

                        finish(
                            reject,
                            error
                        );

                    }
                );

            }
        );

    }


    // ============================================================
    // WEBVIEW
    // ============================================================

    async function createWebview(
        name
    ) {

        return http(
            'POST',
            '/webview/create',
            {
                name
            }
        );

    }


    async function listWebviews() {

        return http(
            'GET',
            '/webview/list'
        );

    }


    async function createWebviewTag(
        name,
        endpointIds
    ) {

        const body = {
            name
        };


        if (
            endpointIds !== undefined
        ) {

            body.endpoint_ids =
                endpointIds;

        }


        return http(
            'POST',
            '/webview/tags/create',
            body
        );

    }


    async function deleteWebviewTags(
        names
    ) {

        return http(
            'POST',
            '/webview/tags/delete',
            {
                names
            }
        );

    }


    async function renameWebviewTag(
        oldName,
        newName
    ) {

        return http(
            'POST',
            '/webview/tags/rename',
            {
                old_name:
                    oldName,

                new_name:
                    newName
            }
        );

    }


    async function listWebviewTags() {

        return http(
            'GET',
            '/webview/tags/list'
        );

    }


    // ============================================================
    // REPO VIEW
    // ============================================================

    async function createRepoview(
        name
    ) {

        return http(
            'POST',
            '/repoview/create',
            {
                name
            }
        );

    }


    async function listRepoviews() {

        return http(
            'GET',
            '/repoview/list'
        );

    }


    async function createRepoviewTag(
        name,
        endpointIds
    ) {

        const body = {
            name
        };


        if (
            endpointIds !== undefined
        ) {

            body.endpoint_ids =
                endpointIds;

        }


        return http(
            'POST',
            '/repoview/tags/create',
            body
        );

    }


    async function deleteRepoviewTags(
        names
    ) {

        return http(
            'POST',
            '/repoview/tags/delete',
            {
                names
            }
        );

    }


    async function renameRepoviewTag(
        oldName,
        newName
    ) {

        return http(
            'POST',
            '/repoview/tags/rename',
            {
                old_name:
                    oldName,

                new_name:
                    newName
            }
        );

    }


    async function listRepoviewTags() {

        return http(
            'GET',
            '/repoview/tags/list'
        );

    }


    // ============================================================
    // REPO EXPORT
    // ============================================================

    async function exportRepo(
        collection,
        filename
    ) {

        return http(
            'GET',
            `/repo/` +
            `${encodeURIComponent(collection)}/` +
            `${encodeURIComponent(filename)}/export`
        );

    }


    // ============================================================
    // REPO IMPORT
    // ============================================================

    async function importRepo(
        collection,
        filename
    ) {

        return http(
            'POST',
            `/repo/` +
            `${encodeURIComponent(collection)}/` +
            `${encodeURIComponent(filename)}/import`
        );

    }


    // ============================================================
    // LOGS
    // ============================================================

    async function getLogs() {

        return http(
            'GET',
            '/logs'
        );

    }


    // ============================================================
    // DASHBOARD
    // ============================================================

    async function getDataViewDashboard() {

        return http(
            'GET',
            '/dataview/dashboard'
        );

    }


    async function getDashboardSnapshot() {

        return http(
            'GET',
            '/dashboard/snapshot'
        );

    }


    async function getDashboardSnapshotHistory() {

        return http(
            'GET',
            '/dashboard/snapshot/history'
        );

    }


    async function getDashboardStatic() {

        return http(
            'GET',
            '/dashboard/static'
        );

    }


    async function getDashboardCrudOperations() {

        return http(
            'GET',
            '/dashboard/crud-operations'
        );

    }


    async function refreshDashboardCrudOperations() {

        return http(
            'POST',
            '/dashboard/crud-operations/refresh'
        );

    }


    async function compareDashboardSnapshots(
        from,
        to
    ) {

        const params =
            new URLSearchParams({

                from,
                to

            });


        return http(
            'GET',
            `/dashboard/compare?${params.toString()}`
        );

    }


    // ============================================================
    // PUBLIC API
    // ============================================================

    window.EdmsAPI = {

        // ----------------------------------------
        // General / Views
        // ----------------------------------------

        getHome,
        getTestView,
        getListView,


        // ----------------------------------------
        // Endpoints
        // ----------------------------------------

        registerEndpoint,
        registerAllEndpoints,
        deleteEndpoint,
        setEndpointAnnotation,


        // ----------------------------------------
        // Test View WebSockets
        // ----------------------------------------

        connectTestView,
        connectEndpointLoader,
        connectBookmarkLoader,
        connectHistoryLoader,

        startTest,
        waitForTestFinished,


        // ----------------------------------------
        // Test View REST
        // ----------------------------------------

        fetchRequest,
        fetchResponse,
        fetchHeaders,

        stopTest,

        saveHistory,
        clearHistory,

        saveBookmark,
        clearBookmarks,


        // ----------------------------------------
        // Active Bookmarks
        // ----------------------------------------

        addActiveBookmark,
        saveActiveBookmark,
        unsaveActiveBookmark,


        // ----------------------------------------
        // Collections
        // ----------------------------------------

        listCollections,
        getCollection,
        createCollection,
        renameCollection,
        deleteCollection,
        setCollectionAnnotation,

        listCollectionEndpoints,
        removeEndpointFromCollection,

        loadCollection,


        // ----------------------------------------
        // Collection Global Tags
        // ----------------------------------------

        createGlobalTag,
        deleteGlobalTags,
        renameGlobalTag,
        listGlobalTags,


        // ----------------------------------------
        // Collection Membership Tags
        // ----------------------------------------

        addMembershipTag,
        removeMembershipTag,
        listMembershipTags,
        listCollectionsByTag,


        // ----------------------------------------
        // Per-Endpoint Tags
        // ----------------------------------------

        listPopularTags,
        listEndpointTags,
        addEndpointTag,
        removeEndpointTag,
        deleteActiveBookmark,



        // ----------------------------------------
        // Data View
        // ----------------------------------------

        deleteDataViewFolder,
        mergeDataViewFolder,
        activateDataViewFolder,


        // ----------------------------------------
        // Webview
        // ----------------------------------------

        createWebview,
        listWebviews,

        createWebviewTag,
        deleteWebviewTags,
        renameWebviewTag,
        listWebviewTags,


        // ----------------------------------------
        // Repo View
        // ----------------------------------------

        createRepoview,
        listRepoviews,

        createRepoviewTag,
        deleteRepoviewTags,
        renameRepoviewTag,
        listRepoviewTags,


        // ----------------------------------------
        // Repo Import / Export
        // ----------------------------------------

        exportRepo,
        importRepo,


        // ----------------------------------------
        // Logs
        // ----------------------------------------

        getLogs,


        // ----------------------------------------
        // Dashboard
        // ----------------------------------------

        getDataViewDashboard,
        getDashboardSnapshot,
        getDashboardSnapshotHistory,
        getDashboardStatic,
        getDashboardCrudOperations,
        refreshDashboardCrudOperations,
        compareDashboardSnapshots

    };

})();