// The service worker. An open native messaging port keeps it running (Chrome 105+).
import { createBridge, handlePopupMessage } from "./bridge.js";

const bridge = createBridge(chrome, (status) => {
  // Updates an open popup; nothing listens when it is closed.
  chrome.runtime.sendMessage({ type: "state", ...status }).catch(() => {});
});

chrome.runtime.onMessage.addListener((message, sender, reply) =>
  handlePopupMessage(bridge, chrome, message, sender, reply)
);
