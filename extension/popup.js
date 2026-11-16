const status = document.getElementById("status");
const show = (reply) => { if (reply?.detail) status.textContent = reply.detail; };
const ask = (type) => chrome.runtime.sendMessage({ type }).then(show, () => {});

chrome.runtime.onMessage.addListener((message, sender) => {
  if (sender.id === chrome.runtime.id && message?.type === "state") show(message);
});
document.getElementById("connect").addEventListener("click", () => ask("connect"));
document.getElementById("disconnect").addEventListener("click", () => ask("disconnect"));
ask("status");
