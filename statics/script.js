window.onload = () => {
    const isWss = window.location.href.includes("https");
    const wsLoc = "ws";
    const webSocketUrl = (isWss ? "wss://" : "ws://") + window.location.host + "/" + wsLoc;
    console.log("socket url: " + webSocketUrl);
    const socket = new WebSocket(webSocketUrl);

    socket.addEventListener('open', function (event) {
        socket.send('Hello Server!');
        onWsConnected();
    });

    socket.addEventListener('message', function (event) {
        console.log('Message from server ', event.data);
    });

    function onWsConnected() {
        setTimeout(() => {
            const obj = { hello: "world" };
            const blob = new Blob([JSON.stringify(obj, null, 2)], {
            type: "application/json",
            });
            console.log("Sending blob over websocket");
            socket.send(blob);
        }, 1000);
        
        setInterval(() => {
            socket.send("Message after 3s from client!");
        }, 3000);
    }
};