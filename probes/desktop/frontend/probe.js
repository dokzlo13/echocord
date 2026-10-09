const status = document.querySelector("#status");

try {
  if (document.readyState !== "complete") {
    await new Promise((resolve) => window.addEventListener("load", resolve, { once: true }));
  }
  const { invoke } = window.__TAURI__.core;
  const acknowledgement = await invoke("frontend_ready");
  if (acknowledgement !== "echocord-desktop-ready") {
    throw new Error("Unexpected Rust readiness acknowledgement");
  }
  status.textContent = "Rust IPC round trip succeeded. Closing shortly…";
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  await new Promise((resolve) => setTimeout(resolve, 1500));
  await invoke("frontend_complete", { acknowledgement });
} catch (error) {
  status.textContent = `Probe failed: ${String(error)}`;
  // Rust's watchdog supplies a nonzero exit if readiness or completion fails.
}
