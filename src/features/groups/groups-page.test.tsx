import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GroupRecordDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
  groupRecord,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("groups page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("shows the empty state, then creates a group with icon and color", async () => {
    const user = userEvent.setup();
    const groups: GroupRecordDto[] = [];
    mockGroupStore(groups);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    expect(
      await screen.findByText("Add a group to organize accounts."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add group" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.type(screen.getByLabelText("Name"), "Emergency");
    await user.click(screen.getByRole("button", { name: "Shield" }));
    await user.click(screen.getByRole("button", { name: "#2563EB" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByRole("heading", { name: "Emergency" }),
    ).toBeInTheDocument();
    expect(commands.createGroup).toHaveBeenCalledWith({
      name: "Emergency",
      iconKey: "shield",
      color: "#2563EB",
      description: null,
    });
  });

  it("updates, archives, shows archived, and restores", async () => {
    const user = userEvent.setup();
    const groups = [
      groupRecord("g-1", "Emergency", { iconKey: "shield", color: "#2563EB" }),
    ];
    mockGroupStore(groups);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    await screen.findByRole("heading", { name: "Emergency" });

    await user.click(screen.getByRole("button", { name: "Edit" }));
    const name = screen.getByLabelText("Name");
    expect(name).toHaveFocus();
    await user.clear(name);
    await user.type(name, "Buffer");
    await user.click(screen.getByRole("button", { name: "Wallet" }));
    await user.click(screen.getByRole("button", { name: "#16A34A" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "Buffer" })).toBeInTheDocument();
    expect(commands.updateGroup).toHaveBeenCalledWith({
      id: "g-1",
      name: "Buffer",
      iconKey: "wallet",
      color: "#16A34A",
      description: null,
    });

    await user.click(screen.getByRole("button", { name: "Archive Buffer" }));
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "Buffer" })).not.toBeInTheDocument();
    });
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "Buffer" })).toBeInTheDocument();
    expect(screen.getByText("Archived")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore Buffer" }));
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "Buffer" })).toBeInTheDocument();
    expect(screen.queryByText("Archived")).not.toBeInTheDocument();
  });

  it("shows server color errors", async () => {
    const user = userEvent.setup();
    mockGroupStore([]);
    vi.mocked(commands.createGroup).mockResolvedValue({
      status: "error",
      error: commandError("VALIDATION_ERROR", "Use a #RRGGBB color.", {
        color: "Use a #RRGGBB color.",
      }),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    await user.click(await screen.findByRole("button", { name: "Add group" }));
    await user.type(screen.getByLabelText("Name"), "Emergency");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText("Please check the highlighted fields."),
    ).toBeInTheDocument();
    expect(screen.getByText("Use a #RRGGBB color.")).toBeInTheDocument();
  });

  it("shows list errors with role=alert", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.listGroups).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("does not save twice while pending", async () => {
    const user = userEvent.setup();
    mockGroupStore([]);
    const pending = deferred<{ status: "ok"; data: GroupRecordDto }>();
    vi.mocked(commands.createGroup).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    await user.click(await screen.findByRole("button", { name: "Add group" }));
    await user.type(screen.getByLabelText("Name"), "Emergency");
    await user.click(screen.getByRole("button", { name: "Save" }));
    const saving = await screen.findByRole("button", { name: "Saving" });
    expect(saving).toBeDisabled();
    await user.click(saving);
    expect(commands.createGroup).toHaveBeenCalledTimes(1);
    pending.resolve({ status: "ok", data: groupRecord("g-1", "Emergency") });
  });

  it("can add a group with the keyboard", async () => {
    const user = userEvent.setup();
    const groups: GroupRecordDto[] = [];
    mockGroupStore(groups);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Groups" }));
    await user.click(await screen.findByRole("button", { name: "Add group" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.keyboard("Emergency");
    const save = screen.getByRole("button", { name: "Save" });
    for (let index = 0; index < 20 && document.activeElement !== save; index += 1) {
      await user.tab();
    }
    expect(save).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "Emergency" }),
    ).toBeInTheDocument();
  });
});

function mockGroupStore(groups: GroupRecordDto[]) {
  vi.mocked(commands.listGroups).mockImplementation(async (input) => ({
    status: "ok",
    data: input.includeArchived
      ? [...groups]
      : groups.filter((group) => group.archivedAt === null),
  }));
  vi.mocked(commands.createGroup).mockImplementation(async (input) => {
    const created = groupRecord(`g-${groups.length + 1}`, input.name, {
      iconKey: input.iconKey,
      color: input.color,
      description: input.description,
      sortOrder: groups.length,
    });
    groups.push(created);
    return { status: "ok", data: created };
  });
  vi.mocked(commands.updateGroup).mockImplementation(async (input) => {
    const group = groups.find((item) => item.id === input.id);
    if (!group) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    group.name = input.name;
    group.iconKey = input.iconKey;
    group.color = input.color;
    group.description = input.description;
    return { status: "ok", data: group };
  });
  vi.mocked(commands.archiveGroup).mockImplementation(async (input) => {
    const group = groups.find((item) => item.id === input.id);
    if (!group) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    group.archivedAt = "2026-08-17T01:00:00.000Z";
    return { status: "ok", data: group };
  });
  vi.mocked(commands.restoreGroup).mockImplementation(async (input) => {
    const group = groups.find((item) => item.id === input.id);
    if (!group) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    group.archivedAt = null;
    return { status: "ok", data: group };
  });
}
