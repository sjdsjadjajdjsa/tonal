// Runs JavaScript in an Electron app's page through the Chrome DevTools Protocol, as if
// the user did it (userGesture), so the benchmark can start playback without touching
// the real mouse or keyboard.
// Usage: node bench/cdp.mjs <port> <expression>
const [port, expression] = process.argv.slice(2);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) {
  console.error("no page target");
  process.exit(1);
}
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id === 1) {
    const result = message.result?.result;
    console.log(JSON.stringify(message.result?.exceptionDetails?.text ?? result?.value ?? result?.description ?? null));
    socket.close();
  }
};
socket.send(
  JSON.stringify({
    id: 1,
    method: "Runtime.evaluate",
    params: { expression, userGesture: true, awaitPromise: true, returnByValue: true },
  }),
);
