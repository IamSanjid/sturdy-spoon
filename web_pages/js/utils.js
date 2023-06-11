const hasBit = function (haystack, needle) {
    return (haystack & needle) === needle;
}

function checkFixedFloat(lhs, rhs) {
    return Math.floor(lhs).toFixed() === Math.floor(rhs).toFixed();
}

function formToJson(form) {
    const VALID_TYPES = {
        "number": (input) => {
            return parseInt(input.value);
        },
        "text": (input) => {
            return input.value;
        },
        "checkbox": (input) => {
            return input.checked;
        },
        "select-one": (input) => {
            const dataType = input.getAttribute("data-type");
            if (dataType === "number") {
                return parseInt(input.value);
            }
            return input.value;
        }
    };

    let data = {};
    for (const item of form.elements) {
        const cnvFn = VALID_TYPES[item.type];
        if (typeof cnvFn === "function") {
            data[item.name] = cnvFn(item);
        }
    }

    return data;
}

function base64Encode(str) {
    return btoa(encodeURIComponent(str));
}
  
function base64Decode(str) {
    return decodeURIComponent(atob(str));
}

function getWsUrl(ws) {
    const isSecured = window.location.href.includes("https");
    const mainUrl = (isSecured ? "wss://" : "ws://") + window.location.host + "/";
    return mainUrl + ws;
}

function getPathUrl(path = "") {
    const isSecured = window.location.href.includes("https");
    const mainUrl = (isSecured ? "https://" : "http://") + window.location.host + "/";
    return mainUrl + path;
}

async function postAsJson(url, data) {
    const dataJsonString = JSON.stringify(data);
    const fetchOptions = {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
            Accept: "application/json",
        },
        body: dataJsonString,
    };
    const res = await fetch(url, fetchOptions);

    if (!res.ok) {
        let error = await res.text();
        throw new Error(error);
    }
    return res.json();
}

function makeWsClient(client) {
    const wsClientObj = {
        inner: client
    };
    function sendText(type, ...args) {
        return wsClientObj.inner.send(new StrPacket(type).addArgs(...args).to_str());
    }
    function close(code, reason) {
        return wsClientObj.inner.close(code, reason);
    }

    wsClientObj.sendText = sendText.bind(wsClientObj);
    wsClientObj.close = close.bind(wsClientObj);
    return wsClientObj;
}

function makeVideoPlayer(videoElement) {
    const videoObj = { 
        inner: videoElement,
    };

    const controls = {
        on: (event, callback) => {
            switch (event) {
                case "seek":
                    videoObj.inner.onseeking = () => {
                        const evt = {
                            offset: videoObj.inner.currentTime
                        }
                        callback(evt);
                    };
                    break;
                case "play":
                    videoObj.inner.onplaying = callback;
                    break;
                case "pause":
                    videoObj.inner.onpause = callback;
                    break;
                case "ready":
                    videoObj.inner.oncanplay = callback;
                    break;
                case "error":
                    videoObj.inner.onerror = callback;
                    break;
            }
        },
        seek: (time) => {
            videoObj.inner.currentTime = time;
        },
        play: () => {
            videoObj.inner.play();
        },
        pause: () => {
            videoObj.inner.pause();
        },
        getCurrentTime: () => {
            return videoObj.inner.currentTime;
        },
        getPosition: () => {
            return videoObj.inner.currentTime;
        },
        getState: () => {
            if (videoObj.inner.seeking) return "seeking";
            if (videoObj.inner.paused) return "paused";
            return "playing";
        }
    }

    const fullVideoObj = {
        ...videoObj,
        ...controls
    };

    return fullVideoObj;
}