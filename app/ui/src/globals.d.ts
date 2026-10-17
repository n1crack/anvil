// Tauri's global API (withGlobalTauri in app/tauri.conf.json) and the hooks the Rust
// side calls with `eval` (see `changed()` in app/src/main.rs).

interface TauriMenuItem {
  popup?: () => Promise<void>;
}

interface TauriGlobal {
  core: {
    invoke<T = any>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  };
  event: {
    listen<T = any>(event: string, handler: (e: { payload: T }) => void): Promise<() => void>;
  };
  menu: any;
  dialog: any;
  window: any;
  app?: { getVersion(): Promise<string> };
}

interface Window {
  __TAURI__: TauriGlobal;
  /** Re-read state; Rust calls it when something changed. */
  refresh?: () => unknown;
  [hook: string]: any;
}
