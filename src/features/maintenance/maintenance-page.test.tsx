import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { commands } from "@/generated/tauri-bindings";
import { router } from "@/app/router";
import { renderReadyApp, resetApp } from "@/test/app-harness";

describe("maintenance page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("does not generate or post work when the page mounts", async () => {
    await renderReadyApp();
    await router.navigate({ to: "/maintenance" });
    expect(
      await screen.findByRole("heading", { name: "Maintenance" }),
    ).toBeInTheDocument();
    expect(commands.generateDuePendingActivities).not.toHaveBeenCalled();
    expect(commands.postPendingActivity).not.toHaveBeenCalled();
  });

  it("generates due items only after an explicit action", async () => {
    vi.mocked(commands.generateDuePendingActivities).mockResolvedValue({
      status: "ok",
      data: { generatedCount: 2, blocked: [], hasMore: false },
    });
    await renderReadyApp();
    const user = userEvent.setup();
    await router.navigate({ to: "/maintenance" });
    await user.click(await screen.findByRole("button", { name: "Generate due items" }));
    expect(commands.generateDuePendingActivities).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Generated 2 pending items.")).toBeInTheDocument();
  });
});
