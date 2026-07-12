import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Users } from "lucide-react";

const WEBSITE_URL = import.meta.env.VITE_WEBSITE_URL || "https://arthdeskapi.ashokitservices.com";

interface NoPersonsScreenProps {
  onRefresh: () => void;
}

export function NoPersonsScreen({ onRefresh }: NoPersonsScreenProps) {
  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <Card className="w-full max-w-md text-center">
        <CardHeader>
          <div className="flex justify-center mb-2">
            <Users className="size-12 text-muted-foreground" />
          </div>
          <CardTitle>No persons found</CardTitle>
          <CardDescription>
            You need to register at least one person on the website before using the app.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Button
            className="w-full"
            onClick={() => openUrl(`${WEBSITE_URL}/account.html`).catch(() => {})}
          >
            Go to website
          </Button>
          <Button variant="outline" className="w-full" onClick={onRefresh}>
            I've added a person — Refresh
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
