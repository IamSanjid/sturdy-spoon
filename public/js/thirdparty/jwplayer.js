(() => {
    "use strict";
    const JW_PLAYER_URL = "//ssl.p.jwpcdn.com/player/v/8.27.1/jwplayer.js";
    const newScript = document.createElement('script');
    newScript.src = JW_PLAYER_URL;

    const injectElement = document.head || document.documentElement;
    injectElement.insertBefore(newScript, injectElement.firstChild);
    newScript.onload = function() {
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
        console.log("JW Player Loaded!");
        newScript.parentNode.removeChild(newScript);
    };
})();
