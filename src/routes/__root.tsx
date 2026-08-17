import { Outlet } from "@tanstack/react-router";

export function RootRoute() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Outlet />
    </div>
  );
}
