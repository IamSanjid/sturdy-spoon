const createRoomForm = document.getElementById("create-room-form");
createRoomForm.onsubmit = async (e) => {
    e.preventDefault();
    const data = formToJson(createRoomForm);
    const respData = await postAsJson(createRoomForm.action, data);
    if (typeof respData.id === "undefined" || typeof respData.ws_path === "undefined") {
        return;
    }
    const encodedRoomId = base64Encode(respData.id);
    console.log(encodedRoomId);
    window.location.assign(getPathUrl("room/" + encodedRoomId));
}