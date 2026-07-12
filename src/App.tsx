import { useEffect, useState } from "react";
import "./App.css";
import { SetupScreen } from "@/components/auth/SetupScreen";
import { LoginScreen } from "@/components/auth/LoginScreen";
import { AppLayout } from "@/components/layout/AppLayout";
import { NoPersonsScreen } from "@/components/auth/NoPersonsScreen";
import { apiGet, apiPost, initBackendListener, setSessionToken, clearSessionToken, setPort, ApiError } from "@/lib/api";
import { type ServerAuthState } from "@/components/auth/ServerLoginDialog";
import { invoke } from "@tauri-apps/api/core";

type AppState = "waiting" | "loading" | "setup" | "locked" | "no-persons" | "select-person" | "unlocked";

export interface PersonRecord {
  person_id: number;
  name: string;
  display_name: string | null;
  masked_pan: string | null;
  subscription_status: string | null;
  subscription_expires_at: string | null;
}

const DEFAULT_SERVER_AUTH: ServerAuthState = {
  logged_in: false, email: "", subscription_status: "", subscription_expires_at: "",
};

function App() {
  const [appState, setAppState]         = useState<AppState>("waiting");
  const [crashMsg, setCrashMsg]         = useState<string | null>(null);
  const [serverAuth, setServerAuth]     = useState<ServerAuthState>(DEFAULT_SERVER_AUTH);
  const [activePerson, setActivePerson] = useState<PersonRecord | null>(null);
  const [persons, setPersons]           = useState<PersonRecord[]>([]);

  useEffect(() => {
    initBackendListener(
      (port) => { setPort(port); setAppState("loading"); checkAuthState(); },
      (reason) => setCrashMsg(reason),
    );
    invoke<number | null>("get_backend_port").then((port) => {
      if (port != null) { setPort(port); setAppState("loading"); checkAuthState(); }
    }).catch(() => {});
  }, []);

  async function checkAuthState() {
    try {
      const status = await apiGet<{ setup: boolean; locked: boolean }>("/auth/status");
      if (!status.setup) { setAppState("setup"); return; }
      if (status.locked) { setAppState("locked"); return; }
      await checkPersons();
    } catch {
      setAppState("locked");
    }
  }

  async function checkPersons() {
    // 1. Try to refresh the remote server JWT (token may have expired during sleep)
    await apiPost("/server-auth/refresh", {}).catch(() => {});

    // 2. Sync persons from server into local cache (falls back to cache on any failure)
    await apiGet("/server-auth/persons").catch(() => {});

    // 3. Server auth state — always succeeds, returns logged_in:false if server unreachable
    const auth = await apiGet<ServerAuthState>("/server-auth/status").catch(() => DEFAULT_SERVER_AUTH);
    setServerAuth(auth);

    // 4. Load local persons — 401 here means the local PIN session was lost (sleep/reload)
    let personList: PersonRecord[];
    try {
      personList = await apiGet<PersonRecord[]>("/persons");
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        clearSessionToken();
        setAppState("locked");
        return;
      }
      personList = [];
    }

    setPersons(personList);
    if (personList.length === 0) {
      setAppState("no-persons");
    } else if (personList.length === 1) {
      setActivePerson(personList[0]);
      setAppState("unlocked");
    } else {
      setAppState("select-person");
    }
  }

  function handleLoginComplete(token: string) {
    setSessionToken(token);
    checkPersons().then(() => {});
  }

  function handleLock() {
    setSessionToken("");
    setActivePerson(null);
    setAppState("locked");
  }

  if (crashMsg) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <div className="text-center space-y-4 p-8 max-w-md">
          <p className="text-lg font-semibold text-destructive">Backend process crashed</p>
          <p className="text-muted-foreground text-sm">{crashMsg}</p>
          <p className="text-muted-foreground text-sm">Please restart the application.</p>
        </div>
      </div>
    );
  }

  if (appState === "waiting" || appState === "loading") {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-muted-foreground">
          {appState === "waiting" ? "Starting backend…" : "Loading…"}
        </p>
      </div>
    );
  }

  if (appState === "setup") {
    return <SetupScreen onComplete={() => checkPersons()} />;
  }

  if (appState === "locked") {
    return <LoginScreen onUnlocked={handleLoginComplete} />;
  }

  if (appState === "no-persons") {
    return <NoPersonsScreen onRefresh={() => checkPersons()} />;
  }

  if (appState === "select-person") {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center p-4">
        <div className="w-full max-w-sm space-y-4">
          <div className="text-center space-y-1">
            <h1 className="text-2xl font-semibold">Who's using the app?</h1>
            <p className="text-sm text-muted-foreground">Select a profile to continue</p>
          </div>
          <div className="space-y-2">
            {persons.map((p) => (
              <button
                key={p.person_id}
                onClick={() => { setActivePerson(p); setAppState("unlocked"); }}
                className="w-full flex items-center gap-3 rounded-xl border bg-card px-4 py-3 text-left hover:bg-accent transition-colors"
              >
                <div className="size-9 rounded-full bg-primary/10 flex items-center justify-center text-primary font-semibold text-sm shrink-0">
                  {(p.display_name || p.name).charAt(0).toUpperCase()}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="font-medium text-sm">{p.display_name || p.name}</div>
                  {p.masked_pan && <div className="text-xs text-muted-foreground">{p.masked_pan}</div>}
                </div>
              </button>
            ))}
          </div>
        </div>
      </div>
    );
  }

  return (
    <AppLayout
      onLock={handleLock}
      serverAuth={serverAuth}
      onServerAuthChange={setServerAuth}
      activePerson={activePerson}
      persons={persons}
      onPersonChange={setActivePerson}
    />
  );
}

export default App;
