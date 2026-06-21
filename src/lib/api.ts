import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Backend port — set when Tauri emits "backend-ready"
// ---------------------------------------------------------------------------

let _port: number | null = null;

export function getPort(): number {
  if (_port === null) throw new Error("Backend not ready yet");
  return _port;
}

export function setPort(port: number): void {
  _port = port;
}

export function initBackendListener(
  onReady: (port: number) => void,
  onCrash: (reason: string) => void,
): void {
  listen<number>("backend-ready", (event) => {
    _port = event.payload;
    onReady(_port);
  });
  listen<string>("backend-crashed", (event) => {
    _port = null;
    onCrash(event.payload ?? "Unknown error");
  });
}

// ---------------------------------------------------------------------------
// Session token — stored only in memory (never persisted to localStorage)
// ---------------------------------------------------------------------------

let _sessionToken: string | null = null;

export function setSessionToken(token: string): void {
  _sessionToken = token;
}

export function clearSessionToken(): void {
  _sessionToken = null;
}

export function hasSession(): boolean {
  return _sessionToken !== null;
}

// ---------------------------------------------------------------------------
// Core fetch wrapper
// ---------------------------------------------------------------------------

class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export async function api<T>(
  path: string,
  body?: unknown,
  method?: string,
): Promise<T> {
  const port = getPort();
  const resolvedMethod = method ?? (body !== undefined ? "POST" : "GET");

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (_sessionToken) {
    headers["X-Session-Token"] = _sessionToken;
  }

  const res = await fetch(`http://127.0.0.1:${port}/api${path}`, {
    method: resolvedMethod,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (!res.ok) {
    let detail = `HTTP ${res.status}`;
    try {
      const json = await res.json();
      detail = json.detail ?? json.message ?? detail;
    } catch {
      detail = await res.text().catch(() => detail);
    }
    throw new ApiError(res.status, detail);
  }

  return res.json() as Promise<T>;
}

// Convenience wrappers
export const apiGet  = <T>(path: string) => api<T>(path);
export const apiPost = <T>(path: string, body: unknown) => api<T>(path, body, "POST");
export const apiPatch= <T>(path: string, body: unknown) => api<T>(path, body, "PATCH");
export const apiPut  = <T>(path: string, body: unknown) => api<T>(path, body, "PUT");
export const apiDel  = <T>(path: string) => api<T>(path, undefined, "DELETE");

// ---------------------------------------------------------------------------
// Native OS APIs (stay as Tauri invoke — file dialogs, file I/O, open path)
// ---------------------------------------------------------------------------

export const native = {
  pickFile: (title?: string): Promise<string | null> =>
    invoke<string | null>("pick_file", { title }),

  pickSavePath: (defaultName?: string): Promise<string | null> =>
    invoke<string | null>("pick_save_path", { defaultName }),

  readTextFile: (path: string): Promise<string> =>
    invoke<string>("read_text_file", { path }),

  writeTextFile: (path: string, content: string): Promise<void> =>
    invoke<void>("write_text_file", { path, content }),

  openPath: (path: string): Promise<void> =>
    invoke<void>("open_path", { path }),
};
