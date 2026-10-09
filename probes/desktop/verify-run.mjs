import { readFile } from "node:fs/promises";

const log = await readFile(process.argv[2], "utf8");
const milestones = [
  "EC-002 runtime_start",
  "EC-002 window_created native_visible=true",
  "EC-002 frontend_ready ipc=true native_visible=true native_size_nonzero=true",
  "EC-002 frontend_round_trip ipc=true",
  "EC-002 window_close_requested",
  "EC-002 window_destroyed",
  "EC-002 runtime_exit",
  "EC-002 lifecycle_complete success=true",
];
let cursor = 0;
for (const milestone of milestones) {
  const position = log.indexOf(milestone, cursor);
  if (position < 0) {
    throw new Error(`Missing or out-of-order runtime milestone: ${milestone}`);
  }
  cursor = position + milestone.length;
}
if (/watchdog_timeout|lifecycle_incomplete/.test(log)) {
  throw new Error("Runtime reported failure");
}
console.log("Windows telemetry verified: visible native window, JS/Rust IPC round trip, destruction, clean exit.");
