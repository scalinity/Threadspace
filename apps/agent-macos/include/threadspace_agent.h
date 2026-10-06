// C ABI of the ThreadspaceAgent Rust core (apps/agent-macos/core/src/lib.rs).
#ifndef THREADSPACE_AGENT_H
#define THREADSPACE_AGENT_H

#include <stdint.h>

// Receives one JSON request from the core. The string is valid only for the
// duration of the call; implementations copy it and return promptly.
typedef void (*ts_bridge_callback)(const char *request_json);

// Starts the core. Returns 0 when running, 75 when another writer holds the
// store, 78 on an unsupported OS, 64 on bad configuration, 70 on a fatal error.
int32_t ts_core_start(const char *config_json, ts_bridge_callback callback);

// Delivers one Apple-layer event (JSON) to the core.
void ts_core_deliver(const char *event_json);

#endif
