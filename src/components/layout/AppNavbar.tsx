import { useState } from "react";
import { apiPost, clearSessionToken } from "@/lib/api";
import { Lock, CheckCircle, AlertCircle, ServerOff, ChevronDown } from "lucide-react";
import { NAV_ITEMS, type Page } from "@/lib/nav";
import { cn } from "@/lib/utils";
import { ServerLoginDialog, type ServerAuthState } from "@/components/auth/ServerLoginDialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type PersonRecord } from "@/App";

const WEBSITE_URL = "https://arthdeskapi.ashokitservices.com";

interface AppNavbarProps {
  currentPage: Page;
  onNavigate: (page: Page) => void;
  onLock: () => void;
  serverAuth: ServerAuthState;
  onServerAuthChange: (state: ServerAuthState) => void;
  activePerson: PersonRecord | null;
  persons: PersonRecord[];
  onPersonChange: (p: PersonRecord) => void;
}

function ServerAuthIndicator({ serverAuth, onServerAuthChange, activePerson }: {
  serverAuth: ServerAuthState;
  onServerAuthChange: (state: ServerAuthState) => void;
  activePerson: PersonRecord | null;
}) {
  const [loginOpen, setLoginOpen] = useState(false);

  if (serverAuth.logged_in) {
    const status = (activePerson?.subscription_status ?? "NONE").toUpperCase();
    const expiry = activePerson?.subscription_expires_at?.slice(0, 10) ?? "—";

    if (status === "ACTIVE") {
      return (
        <span className="flex items-center gap-1 text-xs text-green-600 whitespace-nowrap">
          <CheckCircle className="size-3" />
          Active{expiry !== "—" ? ` until ${expiry}` : ""}
        </span>
      );
    }

    const label = status === "TRIAL"
      ? `Pay by ${expiry}`
      : status === "UNDERPAID"
        ? "Underpaid"
        : status === "PENDING_APPROVAL"
          ? "Pending approval"
          : status === "EXPIRED"
            ? "Payment overdue"
            : "No subscription";

    return (
      <button
        onClick={() => openUrl(`${WEBSITE_URL}/account`).catch(() => {})}
        className="flex items-center gap-1 text-xs text-amber-600 hover:text-amber-800 transition-colors whitespace-nowrap"
      >
        <AlertCircle className="size-3" />
        {label}
      </button>
    );
  }

  return (
    <>
      <button
        onClick={() => setLoginOpen(true)}
        className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors whitespace-nowrap"
      >
        <ServerOff className="size-3" />
        Server: Not logged in
      </button>
      <ServerLoginDialog
        open={loginOpen}
        onOpenChange={setLoginOpen}
        onSuccess={onServerAuthChange}
      />
    </>
  );
}

function PersonSwitcher({ activePerson, persons, onPersonChange }: {
  activePerson: PersonRecord | null;
  persons: PersonRecord[];
  onPersonChange: (p: PersonRecord) => void;
}) {
  const [open, setOpen] = useState(false);
  const displayName = activePerson?.display_name || activePerson?.name || "";

  if (persons.length <= 1) {
    return <span className="text-xs font-medium text-foreground">{displayName}</span>;
  }

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 text-xs font-medium text-foreground hover:text-muted-foreground transition-colors"
      >
        {displayName}
        <ChevronDown className="size-3" />
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div className="absolute right-0 top-6 z-20 bg-popover border rounded-lg shadow-md py-1 min-w-36">
            {persons.map((p) => (
              <button
                key={p.person_id}
                onClick={() => { onPersonChange(p); setOpen(false); }}
                className={cn(
                  "w-full text-left px-3 py-1.5 text-sm hover:bg-accent transition-colors",
                  p.person_id === activePerson?.person_id && "font-medium"
                )}
              >
                {p.display_name || p.name}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export function AppNavbar({ currentPage, onNavigate, onLock, serverAuth, onServerAuthChange, activePerson, persons, onPersonChange }: AppNavbarProps) {
  const handleLock = async () => {
    await apiPost("/auth/lock", {}).catch(() => {});
    clearSessionToken();
    onLock();
  };

  return (
    <header className="flex items-center h-12 border-b bg-background shrink-0 px-4 gap-6">
      <span className="text-sm font-semibold tracking-tight whitespace-nowrap">
        ArthaDesk
      </span>
      <nav className="flex items-center gap-1 flex-1">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.page}
            onClick={() => onNavigate(item.page)}
            className={cn(
              "flex items-center gap-1.5 px-3 h-8 rounded-md text-sm transition-colors",
              currentPage === item.page
                ? "bg-accent text-accent-foreground font-medium"
                : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
            )}
          >
            <item.icon className="size-3.5" />
            {item.label}
          </button>
        ))}
      </nav>
      <div className="flex items-center gap-3 shrink-0">
        <PersonSwitcher activePerson={activePerson} persons={persons} onPersonChange={onPersonChange} />
        <ServerAuthIndicator serverAuth={serverAuth} onServerAuthChange={onServerAuthChange} activePerson={activePerson} />
        <button
          onClick={handleLock}
          className="flex items-center gap-1.5 px-3 h-8 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
        >
          <Lock className="size-3.5" />
          Lock
        </button>
      </div>
    </header>
  );
}
