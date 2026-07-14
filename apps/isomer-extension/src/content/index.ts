/**
 * Content Script
 *
 * Runs on every page, injects the inpage provider, and bridges
 * messages between the page and the background service worker.
 */

// Inject inpage script
const script = document.createElement("script");
script.src = chrome.runtime.getURL("inpage.js");
script.type = "module";
(document.head || document.documentElement).appendChild(script);
script.onload = () => script.remove();

// Bridge messages from page to background
window.addEventListener("message", async (event) => {
  if (event.source !== window || !event.data) return;
  if (event.data.target !== "isomer-companion-content") return;

  const { type, payload, id } = event.data;

  try {
    const response = await chrome.runtime.sendMessage({ type, payload, id });

    window.postMessage(
      {
        target: "isomer-companion-inpage",
        id,
        result: response.result,
        error: response.error,
      },
      "*"
    );
  } catch (error) {
    window.postMessage(
      {
        target: "isomer-companion-inpage",
        id,
        error: (error as Error).message,
      },
      "*"
    );
  }
});

// Notify page that provider is ready
window.postMessage(
  { target: "isomer-companion-inpage", type: "PROVIDER_READY" },
  "*"
);

export {};
