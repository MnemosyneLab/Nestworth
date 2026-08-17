import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { MemberRecordDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
  memberRecord,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("members page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("shows loading status, then lists active members", async () => {
    const user = userEvent.setup();
    const pending = deferred<{ status: "ok"; data: MemberRecordDto[] }>();
    vi.mocked(commands.listMembers).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading…");
    pending.resolve({
      status: "ok",
      data: [memberRecord("m-1", "Walt", 0), memberRecord("m-2", "Spouse", 1)],
    });
    expect(await screen.findByRole("heading", { name: "Walt" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Spouse" })).toBeInTheDocument();
  });

  it("creates, archives, shows archived, and restores a member", async () => {
    const user = userEvent.setup();
    const members = [memberRecord("m-1", "Walt", 0), memberRecord("m-2", "Spouse", 1)];
    mockMemberStore(members);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await screen.findByRole("heading", { name: "Members" });

    await user.click(screen.getByRole("button", { name: "Add member" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.type(screen.getByLabelText("Name"), "Child");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "Child" })).toBeInTheDocument();
    expect(commands.createMember).toHaveBeenCalledWith({
      name: "Child",
      note: null,
    });

    await user.click(screen.getByRole("button", { name: "Archive Child" }));
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "Child" })).not.toBeInTheDocument();
    });
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "Child" })).toBeInTheDocument();
    expect(screen.getByText("Archived")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore Child" }));
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "Child" })).toBeInTheDocument();
    expect(screen.queryByText("Archived")).not.toBeInTheDocument();
  });

  it("updates a member and refreshes the list", async () => {
    const user = userEvent.setup();
    const members = [memberRecord("m-1", "Walt", 0), memberRecord("m-2", "Spouse", 1)];
    mockMemberStore(members);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await screen.findByRole("heading", { name: "Walt" });
    await user.click((await screen.findAllByRole("button", { name: "Edit" }))[0]);
    const name = screen.getByLabelText("Name");
    expect(name).toHaveFocus();
    await user.clear(name);
    await user.type(name, "Walt Wang");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "Walt Wang" })).toBeInTheDocument();
    expect(commands.updateMember).toHaveBeenCalledWith({
      id: "m-1",
      name: "Walt Wang",
      note: null,
    });
  });

  it("shows server field errors on the name input", async () => {
    const user = userEvent.setup();
    const members = [memberRecord("m-1", "Walt", 0)];
    mockMemberStore(members);
    vi.mocked(commands.updateMember).mockResolvedValue({
      status: "error",
      error: commandError("VALIDATION_ERROR", "Name must be between 1 and 80 characters.", {
        name: "Name must be between 1 and 80 characters.",
      }),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await user.click((await screen.findAllByRole("button", { name: "Edit" }))[0]);
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Please check the highlighted fields.")).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(screen.getByText("Name must be between 1 and 80 characters.")).toBeInTheDocument();
  });

  it("focuses name when client validation fails", async () => {
    const user = userEvent.setup();
    mockMemberStore([]);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await user.click(await screen.findByRole("button", { name: "Add member" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(screen.getByText("This field is required.")).toBeInTheDocument();
    expect(commands.createMember).not.toHaveBeenCalled();
  });

  it("does not archive twice while the mutation is pending", async () => {
    const user = userEvent.setup();
    const members = [memberRecord("m-1", "Walt", 0), memberRecord("m-2", "Spouse", 1)];
    mockMemberStore(members);
    const pending = deferred<{ status: "ok"; data: MemberRecordDto }>();
    vi.mocked(commands.archiveMember).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    const archive = await screen.findByRole("button", { name: "Archive Spouse" });
    await user.click(archive);
    const pendingArchive = await screen.findByRole("button", { name: "Archive Spouse" });
    expect(pendingArchive).toBeDisabled();
    await user.click(pendingArchive);
    expect(commands.archiveMember).toHaveBeenCalledTimes(1);
    pending.resolve({
      status: "ok",
      data: { ...members[1], archivedAt: "2026-08-17T01:00:00.000Z" },
    });
  });

  it("does not restore twice while the mutation is pending", async () => {
    const user = userEvent.setup();
    const members = [
      memberRecord("m-1", "Walt", 0),
      memberRecord("m-2", "Spouse", 1, "2026-08-17T01:00:00.000Z"),
    ];
    mockMemberStore(members);
    const pending = deferred<{ status: "ok"; data: MemberRecordDto }>();
    vi.mocked(commands.restoreMember).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await user.click(await screen.findByLabelText("Show archived"));
    const restore = await screen.findByRole("button", { name: "Restore Spouse" });
    await user.click(restore);
    const pendingRestore = await screen.findByRole("button", { name: "Restore Spouse" });
    expect(pendingRestore).toBeDisabled();
    await user.click(pendingRestore);
    expect(commands.restoreMember).toHaveBeenCalledTimes(1);
    pending.resolve({ status: "ok", data: memberRecord("m-2", "Spouse", 1) });
  });

  it("does not save twice while the mutation is pending", async () => {
    const user = userEvent.setup();
    mockMemberStore([]);
    const pending = deferred<{ status: "ok"; data: MemberRecordDto }>();
    vi.mocked(commands.createMember).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await user.click(await screen.findByRole("button", { name: "Add member" }));
    await user.type(screen.getByLabelText("Name"), "Child");
    await user.click(screen.getByRole("button", { name: "Save" }));
    const saving = await screen.findByRole("button", { name: "Saving" });
    expect(saving).toBeDisabled();
    await user.click(saving);
    expect(commands.createMember).toHaveBeenCalledTimes(1);
    pending.resolve({ status: "ok", data: memberRecord("m-3", "Child", 2) });
  });

  it("can add a member with the keyboard", async () => {
    const user = userEvent.setup();
    const members: MemberRecordDto[] = [];
    mockMemberStore(members);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Members" }));
    await user.click(await screen.findByRole("button", { name: "Add member" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.keyboard("Child");
    await user.tab();
    await user.tab();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "Child" })).toBeInTheDocument();
  });
});

function mockMemberStore(members: MemberRecordDto[]) {
  vi.mocked(commands.listMembers).mockImplementation(async (input) => ({
    status: "ok",
    data: input.includeArchived
      ? [...members]
      : members.filter((member) => member.archivedAt === null),
  }));
  vi.mocked(commands.createMember).mockImplementation(async (input) => {
    const created = memberRecord(`m-${members.length + 1}`, input.name, members.length);
    members.push(created);
    return { status: "ok", data: created };
  });
  vi.mocked(commands.updateMember).mockImplementation(async (input) => {
    const member = members.find((item) => item.id === input.id);
    if (!member) {
      return {
        status: "error",
        error: commandError("NOT_FOUND", "missing"),
      };
    }
    member.name = input.name;
    member.note = input.note;
    return { status: "ok", data: member };
  });
  vi.mocked(commands.archiveMember).mockImplementation(async (input) => {
    const member = members.find((item) => item.id === input.id);
    if (!member) {
      return {
        status: "error",
        error: commandError("NOT_FOUND", "missing"),
      };
    }
    member.archivedAt = "2026-08-17T01:00:00.000Z";
    return { status: "ok", data: member };
  });
  vi.mocked(commands.restoreMember).mockImplementation(async (input) => {
    const member = members.find((item) => item.id === input.id);
    if (!member) {
      return {
        status: "error",
        error: commandError("NOT_FOUND", "missing"),
      };
    }
    member.archivedAt = null;
    return { status: "ok", data: member };
  });
}
