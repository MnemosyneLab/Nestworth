import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { InstitutionRecordDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
  institutionRecord,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("institutions page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("shows the empty state, then creates an institution", async () => {
    const user = userEvent.setup();
    const institutions: InstitutionRecordDto[] = [];
    mockInstitutionStore(institutions);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    expect(
      await screen.findByText("Add an institution before you create accounts."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add institution" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.type(screen.getByLabelText("Name"), "DBS");
    await user.type(screen.getByLabelText("Country"), "SG");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "DBS" })).toBeInTheDocument();
    expect(commands.createInstitution).toHaveBeenCalledWith({
      name: "DBS",
      institutionType: null,
      countryCode: "SG",
      website: null,
      note: null,
    });
  });

  it("updates, archives, shows archived, and restores", async () => {
    const user = userEvent.setup();
    const institutions = [institutionRecord("i-1", "DBS", { countryCode: "SG" })];
    mockInstitutionStore(institutions);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    await screen.findByRole("heading", { name: "DBS" });

    await user.click(screen.getByRole("button", { name: "Edit" }));
    const name = screen.getByLabelText("Name");
    expect(name).toHaveFocus();
    await user.clear(name);
    await user.type(name, "DBS Bank");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "DBS Bank" })).toBeInTheDocument();
    expect(commands.updateInstitution).toHaveBeenCalledWith({
      id: "i-1",
      name: "DBS Bank",
      institutionType: null,
      countryCode: "SG",
      website: null,
      note: null,
    });

    await user.click(screen.getByRole("button", { name: "Archive DBS Bank" }));
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "DBS Bank" })).not.toBeInTheDocument();
    });
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "DBS Bank" })).toBeInTheDocument();
    expect(screen.getByText("Archived")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore DBS Bank" }));
    await user.click(screen.getByLabelText("Show archived"));
    expect(await screen.findByRole("heading", { name: "DBS Bank" })).toBeInTheDocument();
    expect(screen.queryByText("Archived")).not.toBeInTheDocument();
  });

  it("shows list errors with role=alert", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.listInstitutions).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("shows server field errors on the country input", async () => {
    const user = userEvent.setup();
    const institutions = [institutionRecord("i-1", "DBS")];
    mockInstitutionStore(institutions);
    vi.mocked(commands.updateInstitution).mockResolvedValue({
      status: "error",
      error: commandError("VALIDATION_ERROR", "Country code must be two uppercase letters.", {
        countryCode: "Country code must be two uppercase letters.",
      }),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    await user.click(await screen.findByRole("button", { name: "Edit" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Please check the highlighted fields.")).toBeInTheDocument();
    expect(screen.getByLabelText("Country")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Country")).toHaveFocus();
    expect(
      screen.getByText("Country code must be two uppercase letters."),
    ).toBeInTheDocument();
  });

  it("does not archive twice while pending", async () => {
    const user = userEvent.setup();
    const institutions = [institutionRecord("i-1", "DBS")];
    mockInstitutionStore(institutions);
    const pending = deferred<{ status: "ok"; data: InstitutionRecordDto }>();
    vi.mocked(commands.archiveInstitution).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    const archive = await screen.findByRole("button", { name: "Archive DBS" });
    await user.click(archive);
    const pendingArchive = await screen.findByRole("button", { name: "Archive DBS" });
    expect(pendingArchive).toBeDisabled();
    await user.click(pendingArchive);
    expect(commands.archiveInstitution).toHaveBeenCalledTimes(1);
    pending.resolve({
      status: "ok",
      data: { ...institutions[0], archivedAt: "2026-08-17T01:00:00.000Z" },
    });
  });

  it("can add an institution with the keyboard", async () => {
    const user = userEvent.setup();
    const institutions: InstitutionRecordDto[] = [];
    mockInstitutionStore(institutions);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Institutions" }));
    await user.click(await screen.findByRole("button", { name: "Add institution" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.keyboard("DBS");
    await user.tab();
    await user.tab();
    await user.tab();
    await user.tab();
    await user.tab();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "DBS" })).toBeInTheDocument();
  });
});

function mockInstitutionStore(institutions: InstitutionRecordDto[]) {
  vi.mocked(commands.listInstitutions).mockImplementation(async (input) => ({
    status: "ok",
    data: input.includeArchived
      ? [...institutions]
      : institutions.filter((institution) => institution.archivedAt === null),
  }));
  vi.mocked(commands.createInstitution).mockImplementation(async (input) => {
    const created = institutionRecord(`i-${institutions.length + 1}`, input.name, {
      institutionType: input.institutionType,
      countryCode: input.countryCode,
      website: input.website,
      note: input.note,
      sortOrder: institutions.length,
    });
    institutions.push(created);
    return { status: "ok", data: created };
  });
  vi.mocked(commands.updateInstitution).mockImplementation(async (input) => {
    const institution = institutions.find((item) => item.id === input.id);
    if (!institution) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    institution.name = input.name;
    institution.institutionType = input.institutionType;
    institution.countryCode = input.countryCode;
    institution.website = input.website;
    institution.note = input.note;
    return { status: "ok", data: institution };
  });
  vi.mocked(commands.archiveInstitution).mockImplementation(async (input) => {
    const institution = institutions.find((item) => item.id === input.id);
    if (!institution) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    institution.archivedAt = "2026-08-17T01:00:00.000Z";
    return { status: "ok", data: institution };
  });
  vi.mocked(commands.restoreInstitution).mockImplementation(async (input) => {
    const institution = institutions.find((item) => item.id === input.id);
    if (!institution) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    institution.archivedAt = null;
    return { status: "ok", data: institution };
  });
}
