import { invoke } from "@tauri-apps/api/core";
import { Lock } from "lucide-react";
import { NAV_ITEMS, type Page } from "@/lib/nav";
import { cn } from "@/lib/utils";

interface AppNavbarProps {
  currentPage: Page;
  onNavigate: (page: Page) => void;
  onLock: () => void;
}

export function AppNavbar({ currentPage, onNavigate, onLock }: AppNavbarProps) {
  const handleLock = async () => {
    await invoke("lock");
    onLock();
  };

  return (
    <header className="flex items-center h-12 border-b bg-background shrink-0 px-4 gap-6">
      {/* Brand */}
      <span className="text-sm font-semibold tracking-tight whitespace-nowrap">
        Portfolio Tracker
      </span>

      {/* Nav items */}
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

      {/* Lock */}
      <button
        onClick={handleLock}
        className="flex items-center gap-1.5 px-3 h-8 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
      >
        <Lock className="size-3.5" />
        Lock
      </button>
    </header>
  );
}
