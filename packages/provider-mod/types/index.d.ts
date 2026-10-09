// The observer's contract: the session values it keeps in `$.state`.
//
// `$.state` is held by the host for the session and survives a hot reload of
// the plugin's code, while module variables do not. Each module load reads
// `epoch` at bootstrap, records it as its predecessor, then writes its own.

declare module 'claude-code' {
  interface PluginState {
    'threadspace-observer': {
      /** The source epoch of the module load that last bootstrapped in this session. */
      epoch: string
    }
  }
}
