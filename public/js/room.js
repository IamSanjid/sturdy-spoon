/* JWPlayer fixes */
localStorage.removeItem("jwplayerLocalId");
localStorage.removeItem("jwplayer.bandwidthEstimate");
const muteOpt = localStorage.getItem("jwplayer.mute");
const wasMute = muteOpt !== null && muteOpt === "true";
if (muteOpt !== null) {
    localStorage.removeItem("jwplayer.mute");
}
const jwDefaults = {
    "aspectratio": "16:9",
    "autostart": false,
    "controls": true,
    "cast": {
        "appid": "00000000"
    },
    "displaydescription": true,
    "displaytitle": true,
    "height": 360,
    "key": "ITWMv7t88JGzI0xPwW8I0+LveiXX9SWbfdmt0ArUSyc=",
    "mute": false,
    "ph": 1,
    "pid": "aVr2lJgW",
    "playbackRateControls": true,
    "preload": "none",
    "repeat": false,
    "stretching": "uniform",
    "width": "100%",
};
jwplayer.defaults = jwDefaults;
/* JWPlayer fixes */

const STATE_PAUSE = 0;
const STATE_PLAY = 1;

const PLAYER_JW = 0;
const PLAYER_NORMAL = 1;

const PERMISSION_RESTRICTED = 0b000;
const PERMISSION_CONTROLLABLE = 0b001;
const PERMISSION_CHANGER = 0b010;

const mainView = document.getElementById("main-view");
const jwplayerView = document.getElementById("jwplayer-view");
const normalPlayerView = document.getElementById("normal-player-view");
const normalPlayer = document.getElementById("normal-player");
const ccFileSelectView = document.getElementById("cc-file-select-view");

const infoCollect = document.getElementById("info-collect");
const roomNameLabel = document.getElementById("room-name");
const joinRoomForm = document.getElementById("room-join-form");
const nameEl = document.getElementById("name");

const ccFileSelect = document.getElementById("ccFileSelect");

const PLAYER_STATES = {
    "playing": STATE_PLAY,
    "paused": STATE_PAUSE,
};

let waitingForUser = {
    "play": true,
    "pause": true,
    "seek": true,
    "url": "",
    "cc_url": "",
    "permission": -1,
    "updateId": null,
    "currentPlayer": PLAYER_JW,
};

let currentCC = null;

let authOpt = localStorage.getItem("local.auth_name");
if (authOpt !== null) {
    joinRoomForm.style = "display: None;";
    nameEl.value = authOpt;
    connectToServer();
}

roomNameLabel.innerText = room_data.name;

const positionToMs = function (pos = undefined) {
    if (typeof pos === "undefined") {
        pos = globalThis.player.getPosition();
    }
    return Math.floor(pos * 1000);
}

const handleMessage = function (message) {
    const [type, data] = new StrPacket("").from_str(message.data);
    console.log(type, data);
    switch (type) {
        case "video_data":
            const video_data = JSON.parse(data[0]);

            mainView.style = "";
            infoCollect.style = "display: None;";

            if (waitingForUser["url"] !== video_data.url
                || waitingForUser["currentPlayer"] !== video_data.current_player) {
                clearCurrentCC();
                let isNewInstance = typeof globalThis.player === "undefined";
                if (!isNewInstance) {
                    delete globalThis.player;
                }
                waitingForUser["currentPlayer"] = video_data.current_player;
                globalThis.player = setupVideoPlayer(video_data.current_player, video_data.url, video_data.state === STATE_PLAY);
                if (video_data.current_player === PLAYER_JW && isNewInstance) {
                    globalThis.player.setMute(wasMute);
                }
                waitingForUser["url"] = video_data.url;
            } else {
                const video_data_time = video_data.time / 1000;
                updateVideoPlayerControls(video_data.permission);
                updatePlayerState(video_data_time, video_data.state);
            }

            globalThis.last_video_data = video_data;
            break;
        case "auth_name":
            localStorage.setItem("local.auth_name", data[0]);
            break;
        case "not_owner":
            localStorage.removeItem("local.owner_auth");
            break;
        case "joined":
            console.log("Somebody joined: ");
            console.log(data);
            break;
        case "left":
            console.log("Somebody left: ");
            console.log(data);
            break;
        default:
            try {
                // received as MS but the player wants in seconds
                const seekTime = parseInt(data[0]) / 1000;
                const position = positionToMs() / 1000;
                switch (type) {
                    case "state":
                        const state = parseInt(data[1]);
                        globalThis.last_video_data.time = seekTime;
                        globalThis.last_video_data.state = state;
                        updatePlayerState(seekTime, state);
                        break;
                    case "seek":
                        globalThis.last_video_data.time = seekTime;
                        if (checkFixedFloat(seekTime, position)) {
                            return;
                        }
                        forcePlayerAction("seek", seekTime);
                        break;
                    case "play":
                        globalThis.last_video_data.time = seekTime;
                        globalThis.last_video_data.state = STATE_PLAY;
                        updatePlayerState(seekTime, STATE_PLAY);
                        break;
                    case "pause":
                        globalThis.last_video_data.time = seekTime;
                        globalThis.last_video_data.state = STATE_PAUSE;
                        updatePlayerState(seekTime, STATE_PAUSE);
                        break;
                }
            } catch (e) {
                console.error(e);
                waitingForUser[type] = true;
            }
            break;
    }
}

const getPlayerState = function () {
    if (typeof globalThis.player === "undefined" || typeof globalThis.last_video_data === "undefined") {
        return null;
    }
    const playerState = PLAYER_STATES[globalThis.player.getState()];
    if (typeof playerState === "undefined") {
        return globalThis.last_video_data.state;
    }
    return playerState;
}

const forcePlayerAction = function (action, ...args) {
    if (typeof globalThis.player === "undefined") {
        return;
    }
    waitingForUser[action] = false;
    globalThis.player[action](...args);
}

const updatePlayerState = function (time, state) {
    if (typeof globalThis.player === "undefined") {
        return;
    }

    const curPos = globalThis.player.getCurrentTime();
    if (!checkFixedFloat(curPos, time)) {
        forcePlayerAction("seek", time);
    }

    const playerState = getPlayerState();
    if (playerState !== state) {
        switch (state) {
            case STATE_PLAY:
                forcePlayerAction("play");
                break;
            case STATE_PAUSE:
                forcePlayerAction("pause");
                break;
        }
    }
}

const seekForward10s = () => {
    globalThis.player.seek(globalThis.player.getPosition() + 10);
};

let hackedJWNextButton = false;
const updateVideoPlayerControls = function (permission) {
    if (waitingForUser.currentPlayer !== PLAYER_JW || typeof globalThis.player === "undefined") {
        return;
    }

    const selectCCButton = '<svg xmlns="http://www.w3.org/2000/svg" class="jw-svg-icon jw-svg-icon-rewind2" viewBox="0 0 490.955 490.955" focusable="false"><path d="M445.767,308.42l-53.374-76.49v-20.656v-11.366V97.241c0-6.669-2.604-12.94-7.318-17.645L312.787,7.301  C308.073,2.588,301.796,0,295.149,0H77.597C54.161,0,35.103,19.066,35.103,42.494V425.68c0,23.427,19.059,42.494,42.494,42.494  h159.307h39.714c1.902,2.54,3.915,5,6.232,7.205c10.033,9.593,23.547,15.576,38.501,15.576c26.935,0-1.247,0,34.363,0  c14.936,0,28.483-5.982,38.517-15.576c11.693-11.159,17.348-25.825,17.348-40.29v-40.06c16.216-3.418,30.114-13.866,37.91-28.811  C459.151,347.704,457.731,325.554,445.767,308.42z M170.095,414.872H87.422V53.302h175.681v46.752  c0,16.655,13.547,30.209,30.209,30.209h46.76v66.377h-0.255v0.039c-17.685-0.415-35.529,7.285-46.934,23.46l-61.586,88.28  c-11.965,17.134-13.387,39.284-3.722,57.799c7.795,14.945,21.692,25.393,37.91,28.811v19.842h-10.29H170.095z M410.316,345.771  c-2.03,3.866-5.99,6.271-10.337,6.271h-0.016h-32.575v83.048c0,6.437-5.239,11.662-11.659,11.662h-0.017H321.35h-0.017  c-6.423,0-11.662-5.225-11.662-11.662v-83.048h-32.574h-0.016c-4.346,0-8.308-2.405-10.336-6.271  c-2.012-3.866-1.725-8.49,0.783-12.07l61.424-88.064c2.189-3.123,5.769-4.984,9.57-4.984h0.017c3.802,0,7.38,1.861,9.568,4.984  l61.427,88.064C412.04,337.28,412.328,341.905,410.316,345.771z"></path></svg>';
    const selectCCButtonTooltip = 'Select local Subtitle/CC file..';
    const selectCCButtonName = 'jw-select-cc';

    const forwardButton = '<svg xmlns="http://www.w3.org/2000/svg" class="jw-svg-icon jw-svg-icon-rewind2" viewBox="0 0 240 240" focusable="false"><path d="m 25.993957,57.778 v 125.3 c 0.03604,2.63589 2.164107,4.76396 4.8,4.8 h 62.7 v -19.3 h -48.2 v -96.4 H 160.99396 v 19.3 c 0,5.3 3.6,7.2 8,4.3 l 41.8,-27.9 c 2.93574,-1.480087 4.13843,-5.04363 2.7,-8 -0.57502,-1.174985 -1.52502,-2.124979 -2.7,-2.7 l -41.8,-27.9 c -4.4,-2.9 -8,-1 -8,4.3 v 19.3 H 30.893957 c -2.689569,0.03972 -4.860275,2.210431 -4.9,4.9 z m 163.422413,73.04577 c -3.72072,-6.30626 -10.38421,-10.29683 -17.7,-10.6 -7.31579,0.30317 -13.97928,4.29374 -17.7,10.6 -8.60009,14.23525 -8.60009,32.06475 0,46.3 3.72072,6.30626 10.38421,10.29683 17.7,10.6 7.31579,-0.30317 13.97928,-4.29374 17.7,-10.6 8.60009,-14.23525 8.60009,-32.06475 0,-46.3 z m -17.7,47.2 c -7.8,0 -14.4,-11 -14.4,-24.1 0,-13.1 6.6,-24.1 14.4,-24.1 7.8,0 14.4,11 14.4,24.1 0,13.1 -6.5,24.1 -14.4,24.1 z m -47.77056,9.72863 v -51 l -4.8,4.8 -6.8,-6.8 13,-12.99999 c 3.02543,-3.03598 8.21053,-0.88605 8.2,3.4 v 62.69999 z"></path></svg>';
    const forwardButtonTooltip = 'Forward 10 Seconds';
    const forwardButtonName = 'jw-forward';

    const slider = document.getElementsByClassName('jw-slider-time jw-background-color jw-reset jw-slider-horizontal jw-reset');
    const playback = document.getElementsByClassName('jw-icon jw-icon-inline jw-button-color jw-reset jw-icon-playback');
    const rewinds = document.getElementsByClassName('jw-icon jw-icon-rewind jw-button-color jw-reset');

    const bigPlayButton = document.getElementsByClassName('jw-display-icon-container jw-display-icon-display jw-reset');
    const startPlayButton = document.getElementsByClassName('jw-icon jw-icon-display jw-button-color jw-reset');
    const nextButton = document.querySelector('.jw-display-icon-next');

    let permissionStyle = "";

    const controllable = hasBit(permission, PERMISSION_CONTROLLABLE);

    globalThis.player.removeButton(forwardButtonName);
    globalThis.player.removeButton(selectCCButtonName);
    globalThis.player.addButton(selectCCButton, selectCCButtonTooltip, () => {
        ccFileSelect.click();
    }, selectCCButtonName);

    if (!controllable) {
        permissionStyle = "display: None";
    } else {
        globalThis.player.addButton(forwardButton, forwardButtonTooltip, seekForward10s, forwardButtonName);

        if (!hackedJWNextButton && nextButton !== null) {
            const nextButtonIcon = nextButton.querySelector('.jw-icon-next');
            nextButtonIcon.setAttribute('aria-label', forwardButtonTooltip);
            nextButtonIcon.innerHTML =
                '<svg xmlns="http://www.w3.org/2000/svg" class="jw-svg-icon" viewBox="0 0 240 240" focusable="false"><path d="m 25.993957,57.778 v 125.3 c 0.03604,2.63589 2.164107,4.76396 4.8,4.8 h 62.7 v -19.3 h -48.2 v -96.4 H 160.99396 v 19.3 c 0,5.3 3.6,7.2 8,4.3 l 41.8,-27.9 c 2.93574,-1.480087 4.13843,-5.04363 2.7,-8 -0.57502,-1.174985 -1.52502,-2.124979 -2.7,-2.7 l -41.8,-27.9 c -4.4,-2.9 -8,-1 -8,4.3 v 19.3 H 30.893957 c -2.689569,0.03972 -4.860275,2.210431 -4.9,4.9 z m 163.422413,73.04577 c -3.72072,-6.30626 -10.38421,-10.29683 -17.7,-10.6 -7.31579,0.30317 -13.97928,4.29374 -17.7,10.6 -8.60009,14.23525 -8.60009,32.06475 0,46.3 3.72072,6.30626 10.38421,10.29683 17.7,10.6 7.31579,-0.30317 13.97928,-4.29374 17.7,-10.6 8.60009,-14.23525 8.60009,-32.06475 0,-46.3 z m -17.7,47.2 c -7.8,0 -14.4,-11 -14.4,-24.1 0,-13.1 6.6,-24.1 14.4,-24.1 7.8,0 14.4,11 14.4,24.1 0,13.1 -6.5,24.1 -14.4,24.1 z m -47.77056,9.72863 v -51 l -4.8,4.8 -6.8,-6.8 13,-12.99999 c 3.02543,-3.03598 8.21053,-0.88605 8.2,3.4 v 62.69999 z"></path></svg>';
            nextButton.onclick = seekForward10s;
            hackedJWNextButton = true;
        }
    }

    if (slider.length > 0) {
        slider[0].style = permissionStyle;
        playback[0].style = permissionStyle;
        for (let rewind of rewinds) {
            rewind.style = permissionStyle;
        }
    }

    if (bigPlayButton.length > 0) {
        bigPlayButton[0].style = permissionStyle;
    }

    if (startPlayButton.length > 0) {
        startPlayButton[0].style = permissionStyle;
    }

    if (nextButton !== null) {
        nextButton.style = permissionStyle;
    }

    waitingForUser["permission"] = permission;
}

const initializePlayerEvents = function (player) {
    player['on']('ready', function () {
        const video_data = globalThis.last_video_data;
        const video_data_time = video_data.time / 1000;
        const video_data_state = video_data.state;

        const curPos = player.getPosition();

        updateVideoPlayerControls(video_data.permission);
        if (curPos <= 1.0 && !checkFixedFloat(curPos, video_data_time)) {
            forcePlayerAction("seek", video_data_time);
        }

        if (video_data_state === STATE_PLAY) {
            forcePlayerAction("play");
        }

        if (waitingForUser["cc_url"] !== video_data.cc_url) {
            addCurrentCC(video_data.cc_url, "Current CC");
            waitingForUser["cc_url"] = video_data.cc_url;
        }
    });
    player['on']("error", function () {
        console.error(arguments);
    });
    player['on']("seek", function (evt) {
        const currentEvt = "seek";

        const video_data = globalThis.last_video_data;

        if (!waitingForUser[currentEvt]) {
            waitingForUser[currentEvt] = true;
            return;
        }
        if (!hasBit(video_data.permission, PERMISSION_CONTROLLABLE)) {
            forcePlayerAction("seek", evt.position);
            return;
        }
        globalThis.ws_client.sendText(currentEvt, evt.offset);
    });
    player['on']("play", function () {
        const currentEvt = "play";

        const video_data = globalThis.last_video_data;
        const last_video_data_state = video_data.state;

        updateVideoPlayerControls(video_data.permission);

        if (!waitingForUser[currentEvt]) {
            waitingForUser[currentEvt] = true;
            return;
        }

        if (last_video_data_state !== STATE_PLAY && !hasBit(video_data.permission, PERMISSION_CONTROLLABLE)) {
            forcePlayerAction("pause");
            return;
        }

        globalThis.ws_client.sendText(currentEvt, player.getPosition());
    });
    player['on']("pause", function () {
        const currentEvt = "pause";

        const video_data = globalThis.last_video_data;
        const last_video_data_state = video_data.state;

        updateVideoPlayerControls(video_data.permission);

        if (!waitingForUser[currentEvt]) {
            waitingForUser[currentEvt] = true;
            return;
        }

        if (last_video_data_state !== STATE_PAUSE && !hasBit(video_data.permission, PERMISSION_CONTROLLABLE)) {
            forcePlayerAction("play");
            return;
        }

        if (arguments.length > 0 && typeof arguments[0].pauseReason === "undefined") {
            return;
        }

        globalThis.ws_client.sendText(currentEvt, player.getPosition());
    });

    if (waitingForUser["updateId"] !== null) {
        clearInterval(waitingForUser["updateId"]);
    }

    waitingForUser["updateId"] = setInterval(function () {
        if (typeof globalThis.player === "undefined" || typeof globalThis.ws_client === "undefined") {
            return;
        }
        globalThis.ws_client.sendText("state", globalThis.player.getCurrentTime(), getPlayerState());
    }, 30 * 1000);
}

const setupVideoPlayer = function (index, url, autostart = false) {
    switch (index) {
        case PLAYER_JW:
            jwplayerView.style = "";
            normalPlayerView.style = "display: none;";
            ccFileSelectView.style = "display: none;";
            return setupJwVideoPlayer(url, autostart);
        case PLAYER_NORMAL:
            ccFileSelectView.style = "";
            normalPlayerView.style = "";
            jwplayerView.style = "display: none;";
            return setupNormalVideoPlayer(url);
        default:
            throw new Error("Cannot setup unknown player.");
    }
}

const setupJwVideoPlayer = function (url, autostart = false) {
    const player = jwplayer("player-div");
    const player_config = {
        playbackRateControls: [1],
        preload: "auto",
        sources: [
            {
                aspectratio: "16:9",
                file: url,
                label: "hls P",
                preload: "auto",
                type: "mp4",
                width: "100%",
            }
        ],
        autostart: autostart
    };

    player.setup(player_config);
    initializePlayerEvents(player);

    return player;
}

const setupNormalVideoPlayer = function (url) {
    const player = makeVideoPlayer(normalPlayer);
    initializePlayerEvents(player);
    normalPlayer.src = url;
    normalPlayer.type = 'video/mp4';
    return player;
}

const clearCurrentCC = function () {
    if (typeof globalThis.player === "undefined") {
        currentCC = null;
        return;
    }
    const lastState = globalThis.player.getState();
    const lastTime = globalThis.player.getCurrentTime();

    if (waitingForUser["currentPlayer"] === PLAYER_JW) {
        const currentPlaylist = globalThis.player.getPlaylistItem();
        if (currentCC !== null && typeof currentCC === "number" && currentPlaylist.tracks.length > currentCC) {
            currentPlaylist.tracks[currentCC] = {};
        }
        globalThis.player.load(currentPlaylist);
    } else if (waitingForUser["currentPlayer"] === PLAYER_NORMAL) {
        if (currentCC !== null) {
            normalPlayer.removeChild(currentCC);
        }
    }

    setTimeout(() => {
        if (typeof globalThis.player === "undefined") {
            return;
        }
        forcePlayerAction("seek", lastTime);
        if (lastState === "playing") {
            forcePlayerAction("play");
        }
    }, 250);
    currentCC = null;
}

const addCurrentCC = function (url, fileName) {
    const lastState = globalThis.player.getState();
    const lastTime = globalThis.player.getCurrentTime();

    if (waitingForUser["currentPlayer"] === PLAYER_JW) {
        const currentPlaylist = globalThis.player.getPlaylistItem();
        if (currentCC === null) {
            currentCC = currentPlaylist.tracks.length;
            currentPlaylist.tracks.push({
                file: url,
                label: fileName,
                kind: "captions"
            });
        } else {
            currentPlaylist.tracks[currentCC] = {
                file: url,
                label: fileName,
                kind: "captions"
            };
        }
        globalThis.player.load(currentPlaylist);
    } else if (waitingForUser["currentPlayer"] === PLAYER_NORMAL) {
        if (currentCC !== null) {
            normalPlayer.removeChild(currentCC);
        }

        currentCC = document.createElement("track");
        currentCC.kind = "captions";
        currentCC.label = fileName;
        currentCC.src = url;
        currentCC.addEventListener("load", function () {
            this.mode = "showing";
            for (track of normalPlayer.textTracks) {
                if (track === currentCC) {
                    track.mode = "showing";
                    break;
                }
            }
        });

        normalPlayer.appendChild(currentCC);
    }

    setTimeout(() => {
        if (typeof globalThis.player === "undefined") {
            return;
        }
        forcePlayerAction("seek", lastTime);
        if (lastState === "playing") {
            forcePlayerAction("play");
        }
    }, 250);
}

const addLocalCC = function () {
    if (typeof globalThis.player === "undefined") {
        return;
    }
    if (typeof ccFileSelect.files === "undefined" || ccFileSelect.files.length <= 0) {
        alert("No file selected!");
        return;
    }

    const file = ccFileSelect.files[0];
    //const fileName = file.name.split('.').slice(0, -1).join('.');
    const tmppath = URL.createObjectURL(file);
    addCurrentCC(tmppath, "Current CC");
}

function connectToServer() {
    if (typeof room_data.ws_path === "undefined") {
        return;
    }
    const name = nameEl.value;
    const client = new WebSocket(getWsUrl(room_data.ws_path));
    client.onopen = (e) => {
        console.log("ws opened: ", e);
        const owner_auth = localStorage.getItem("local.owner_auth");
        let packet = new StrPacket("join_room").addArgs(room_data.room_id, name);
        if (owner_auth !== null) {
            packet.addArgs(owner_auth);
        }
        client.send(packet.to_str());
    }
    client.onclose = (e) => {
        console.log("ws closed: ", e);
        if (authOpt !== null) {
            localStorage.removeItem("local.auth");
        }
        const reason = e.reason.length > 0 ? e.reason : "Forced closed. Code: " + e.code;
        alert("Error: " + reason);
        window.location.assign(getPathUrl());
    };
    client.onerror = (e) => {
        console.log("ws error: ", e);
        if (authOpt !== null) {
            localStorage.removeItem("local.auth");
        }
        const reason = e.reason.length > 0 ? e.reason : "Forced closed. Code: " + e.code;
        alert("Error: " + reason);
        window.location.assign(getPathUrl());
    };
    client.onmessage = (e) => {
        handleMessage(e);
    }
    globalThis.ws_client = makeWsClient(client);
}

joinRoomForm.onsubmit = (e) => {
    e.preventDefault();
    connectToServer();
};

ccFileSelect.onchange = (_) => {
    addLocalCC();
};