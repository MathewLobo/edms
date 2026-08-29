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

async function http(method, path, body) {

    const response = await fetch(
        `${API_BASE}${path}`,
        {
            method,

            headers: {
                'Content-Type': 'application/json'
            },

            body:
                body === undefined
                    ? undefined
                    : JSON.stringify(body)
        }
    );


    const text =
        await response.text();


    let data;


    try {

        data =
            JSON.parse(text);

    } catch {

        data =
            text;

    }


    return {

        status: response.status,

        ok: response.ok,

        data

    };

}


// ============================================================
// R1 — REGISTER ENDPOINT
// ============================================================

async function registerEndpoint(
    endpointId,
    endpointUrl,
    method
) {

    return http(
        'POST',
        '/endpoints/create',
        {
            endpoint_id: endpointId,

            endpoint_str: endpointUrl,

            method: method
        }
    );

}


// ============================================================
// WS — CONNECT TEST VIEW
// ============================================================

function connectTestView() {

    return new WebSocket(
        `${WS_BASE}/test-view/run`
    );

}


// ============================================================
// WS — START TEST
// ============================================================

function startTest(
    ws,
    endpointId,
    method,
    requestJson = {},
    timeoutMs = 30000,
    tickIntervalMs = 500
) {

    const message = {

        type: 'run',

        payload: {

            endpoint_id:
                endpointId,

            method:
                method,

            request_json:
                requestJson,

            timeout_ms:
                timeoutMs,

            tick_interval_ms:
                tickIntervalMs

        }

    };


    ws.send(
        JSON.stringify(message)
    );

}


// ============================================================
// WS — WAIT FOR COMPLETION
// ============================================================

function waitForTestFinished(ws) {

    return new Promise(
        (resolve, reject) => {

            function handleMessage(data) {

                try {

                    const message =
                        JSON.parse(
                            data.toString()
                        );


                    const event =
                        message.event;


                    if (!event) {
                        return;
                    }


                    // ----------------------------------------
                    // TEST FINISHED
                    // ----------------------------------------

                    if (
                        event.type ===
                        'TestFinished'
                    ) {

                        cleanup();


                        resolve(
                            event
                        );

                    }


                    // ----------------------------------------
                    // TIMEOUT
                    // ----------------------------------------

                    else if (
                        event.type ===
                        'TestTimeout'
                    ) {

                        cleanup();


                        reject(
                            new Error(
                                'Backend reported TestTimeout'
                            )
                        );

                    }


                    // ----------------------------------------
                    // ERROR
                    // ----------------------------------------

                    else if (
                        event.type ===
                        'Error'
                    ) {

                        cleanup();


                        reject(
                            new Error(
                                event.payload?.message ||
                                'Backend returned Error'
                            )
                        );

                    }

                } catch (error) {

                    cleanup();

                    reject(error);

                }

            }


            function handleError(error) {

                cleanup();

                reject(error);

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
// R2 — FETCH SAVED RESPONSE
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
// PUBLIC API
// ============================================================

window.EdmsAPI = {

    registerEndpoint,

    connectTestView,

    startTest,

    waitForTestFinished,

    fetchResponse

};


})();