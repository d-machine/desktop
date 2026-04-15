import { invoke } from "@tauri-apps/api/core";
import { Lock } from "lucide-react";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { NAV_ITEMS, type Page } from "@/lib/nav";

interface AppSidebarProps {
  currentPage: Page;
  onNavigate: (page: Page) => void;
  onLock: () => void;
  portfolioName?: string;
}

export function AppSidebar({ currentPage, onNavigate, onLock, portfolioName }: AppSidebarProps) {
  const mainItems = NAV_ITEMS.filter((i) => i.group === "main");
  const reportItems = NAV_ITEMS.filter((i) => i.group === "reports");
  const settingsItems = NAV_ITEMS.filter((i) => i.group === "settings");

  const handleLock = async () => {
    await invoke("lock");
    onLock();
  };

  return (
    <Sidebar>
      {/* Header — app name + active portfolio */}
      <SidebarHeader className="px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <span className="text-sm font-semibold tracking-tight">Portfolio Tracker</span>
          {portfolioName && (
            <span className="text-xs text-muted-foreground truncate">{portfolioName}</span>
          )}
        </div>
      </SidebarHeader>

      <SidebarSeparator />

      <SidebarContent>
        {/* Main */}
        <SidebarGroup>
          <SidebarMenu>
            {mainItems.map((item) => (
              <SidebarMenuItem key={item.page}>
                <SidebarMenuButton
                  isActive={currentPage === item.page}
                  onClick={() => onNavigate(item.page)}
                >
                  <item.icon className="size-4" />
                  <span>{item.label}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarSeparator />

        {/* Reports */}
        <SidebarGroup>
          <SidebarGroupLabel>Reports</SidebarGroupLabel>
          <SidebarMenu>
            {reportItems.map((item) => (
              <SidebarMenuItem key={item.page}>
                <SidebarMenuButton
                  isActive={currentPage === item.page}
                  onClick={() => onNavigate(item.page)}
                >
                  <item.icon className="size-4" />
                  <span>{item.label}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarSeparator />

        {/* Settings */}
        <SidebarGroup>
          <SidebarMenu>
            {settingsItems.map((item) => (
              <SidebarMenuItem key={item.page}>
                <SidebarMenuButton
                  isActive={currentPage === item.page}
                  onClick={() => onNavigate(item.page)}
                >
                  <item.icon className="size-4" />
                  <span>{item.label}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>

      {/* Footer — lock button */}
      <SidebarFooter className="p-2">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={handleLock} className="text-muted-foreground">
              <Lock className="size-4" />
              <span>Lock</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
