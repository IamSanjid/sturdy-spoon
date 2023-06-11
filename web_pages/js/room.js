/* JWPlayer fixes */
localStorage.removeItem("jwplayerLocalId");
localStorage.removeItem("jwplayer.bandwidthEstimate");
const muteOpt = localStorage.getItem("jwplayer.mute");
const wasMute = muteOpt !== null && muteOpt === "true";
if (muteOpt !== null) {
    localStorage.removeItem("jwplayer.mute");
}
/* JWPlayer fixes */

const STATE_PAUSE = 0;
const STATE_PLAY = 1;

const PERMISSION_RESTRICTED = 0b000;
const PERMISSION_CONTROLLABLE = 0b001;
const PERMISSION_CHANGER = 0b010;

const mainView = document.getElementById("main-view");
const infoCollect = document.getElementById("info-collect");
const roomNameLabel = document.getElementById("room-name");
const joinRoomForm = document.getElementById("room-join-form");
const nameEl = document.getElementById("name");

const fileSelect = document.getElementById("fileSelect");
const addSubBtn = document.getElementById("addSubBtn");

const PLAYER_STATES = {
    "playing": STATE_PLAY,
    "paused": STATE_PAUSE,
};

let waitingForUser = {
    "play": true,
    "pause": true,
    "seek": true,
    "url": "",
    "permission": -1,
    "updateId": null,
};

let authOpt = localStorage.getItem("local.auth");
if (authOpt !== null) {
    const [auth_str, expiration_str] = authOpt.split("|.|");
    const expiration = parseInt(expiration_str);
    if (Date.now() > expiration) {
        localStorage.removeItem("local.auth");
        authOpt = null;    
    } else {
        authOpt = auth_str;
        infoCollect.style = "display: None;";
        connectToServer();
    }
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

            if (waitingForUser["url"] !== video_data.url) {
                let isNewInstance = typeof globalThis.player === "undefined";
                if (!isNewInstance) {
                    delete globalThis.player;
                }
                globalThis.player = setupVideoPlayer(video_data.url, video_data.state === STATE_PLAY);
                if (isNewInstance) {
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
        case "auth":
            localStorage.setItem("local.auth", data[0] + "|.|" + data[1]);
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

const updateVideoPlayerControls = function (permission) {
    if (typeof globalThis.player === "undefined") {
        return;
    }

    const forwardButton = '<svg xmlns="http://www.w3.org/2000/svg" class="jw-svg-icon jw-svg-icon-rewind2" viewBox="0 0 240 240" focusable="false"><path d="m 25.993957,57.778 v 125.3 c 0.03604,2.63589 2.164107,4.76396 4.8,4.8 h 62.7 v -19.3 h -48.2 v -96.4 H 160.99396 v 19.3 c 0,5.3 3.6,7.2 8,4.3 l 41.8,-27.9 c 2.93574,-1.480087 4.13843,-5.04363 2.7,-8 -0.57502,-1.174985 -1.52502,-2.124979 -2.7,-2.7 l -41.8,-27.9 c -4.4,-2.9 -8,-1 -8,4.3 v 19.3 H 30.893957 c -2.689569,0.03972 -4.860275,2.210431 -4.9,4.9 z m 163.422413,73.04577 c -3.72072,-6.30626 -10.38421,-10.29683 -17.7,-10.6 -7.31579,0.30317 -13.97928,4.29374 -17.7,10.6 -8.60009,14.23525 -8.60009,32.06475 0,46.3 3.72072,6.30626 10.38421,10.29683 17.7,10.6 7.31579,-0.30317 13.97928,-4.29374 17.7,-10.6 8.60009,-14.23525 8.60009,-32.06475 0,-46.3 z m -17.7,47.2 c -7.8,0 -14.4,-11 -14.4,-24.1 0,-13.1 6.6,-24.1 14.4,-24.1 7.8,0 14.4,11 14.4,24.1 0,13.1 -6.5,24.1 -14.4,24.1 z m -47.77056,9.72863 v -51 l -4.8,4.8 -6.8,-6.8 13,-12.99999 c 3.02543,-3.03598 8.21053,-0.88605 8.2,3.4 v 62.69999 z"></path></svg>';
    const forwardButtonTooltip = 'Forward 10 Seconds';
    const forwardButtonName = 'Next 10s';

    const slider = document.getElementsByClassName('jw-slider-time jw-background-color jw-reset jw-slider-horizontal jw-reset');
    const playback = document.getElementsByClassName('jw-icon jw-icon-inline jw-button-color jw-reset jw-icon-playback');
    const rewinds = document.getElementsByClassName('jw-icon jw-icon-rewind jw-button-color jw-reset');

    const bigPlayButton = document.getElementsByClassName('jw-display-icon-container jw-display-icon-display jw-reset');
    const startPlayButton = document.getElementsByClassName('jw-icon jw-icon-display jw-button-color jw-reset');

    let permissionStyle = "";

    const controllable = hasBit(permission, PERMISSION_CONTROLLABLE);
    if (!controllable) {
        globalThis.player.removeButton(forwardButtonName);
        permissionStyle = "display: None";
    } else {
        globalThis.player.addButton(forwardButton, forwardButtonTooltip, function () {
            globalThis.player.seek(globalThis.player.getPosition() + 10);
        }, forwardButtonName);
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

    waitingForUser["permission"] = permission;
}

const setupVideoPlayer = function (url, autostart = false) {
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

    return player;
}

function connectToServer() {
    if (typeof room_data.ws_path === "undefined") {
        return;
    }
    const name = nameEl.value;
    const client = new WebSocket(getWsUrl(room_data.ws_path));
    client.onopen = (e) => {
        console.log("ws opened: ", e);
        let packet; 
        if (authOpt !== null) {
            packet = new StrPacket("auth_join").addArgs(authOpt);
        } else {
            packet = new StrPacket("join_room").addArgs(room_data.room_id, name);
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

addSubBtn.onclick = () => {
    if (typeof globalThis.player === "undefined") {
        return;
    }
    if (typeof fileSelect.files === "undefined" || fileSelect.files.length <= 0) {
        alert("No file selected!");
        return;
    }
    const file = fileSelect.files[0];
    var tmppath = URL.createObjectURL(file);
    const currentPlaylist = globalThis.player.getPlaylistItem();
    const lastState = globalThis.player.getState();
    const lastTime = globalThis.player.getCurrentTime();
    currentPlaylist.tracks = [{
        file: tmppath,
        label: file.name.split('.').slice(0, -1).join('.'),
        kind: "captions"
    }];

    globalThis.player.load(currentPlaylist);
    setTimeout(() => {
        if (typeof globalThis.player === "undefined") {
            return;
        }
        globalThis.player.seek(lastTime);
        if (lastState === "playing") {
            globalThis.player.play();
        }
    }, 250);
}

joinRoomForm.onsubmit = (e) => {
    e.preventDefault();
    connectToServer();
};