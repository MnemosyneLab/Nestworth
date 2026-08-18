import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AppSettingsDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import { deferred, readyBootstrap, renderReadyApp, resetApp } from "@/test/app-harness";

describe("settings general page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("switches language without remounting the app", async () => {
    const user = userEvent.setup();
    const settings: AppSettingsDto = {
      language: "en",
      appearance: "light",
      lastHouseholdId: "hh-1",
    };
    mockSettings(settings);
    await renderReadyApp();
    mockSettings(settings);
    expect(screen.getByRole("link", { name: "Overview" })).toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "Settings" }));
    await user.selectOptions(await screen.findByLabelText("Language"), "zh-CN");
    await waitFor(() => {
      expect(commands.updateSettings).toHaveBeenCalledWith({
        language: "zh-CN",
        appearance: "light",
      });
    });
    expect(await screen.findByRole("link", { name: "总览" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "设置" })).toBeInTheDocument();
  });

  it("switches appearance without remounting the app", async () => {
    const user = userEvent.setup();
    const settings: AppSettingsDto = {
      language: "en",
      appearance: "light",
      lastHouseholdId: "hh-1",
    };
    mockSettings(settings);
    await renderReadyApp();
    mockSettings(settings);
    expect(document.documentElement).not.toHaveClass("dark");
    await user.click(screen.getByRole("link", { name: "Settings" }));
    await user.selectOptions(await screen.findByLabelText("Appearance"), "Dark");
    await waitFor(() => {
      expect(commands.updateSettings).toHaveBeenCalledWith({
        language: "en",
        appearance: "dark",
      });
      expect(document.documentElement).toHaveClass("dark");
    });
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
  });

  it("can change language from a labeled control", async () => {
    const user = userEvent.setup();
    const settings: AppSettingsDto = {
      language: "en",
      appearance: "light",
      lastHouseholdId: "hh-1",
    };
    mockSettings(settings);
    await renderReadyApp();
    mockSettings(settings);
    await user.click(screen.getByRole("link", { name: "Settings" }));
    const language = await screen.findByLabelText("Language");
    language.focus();
    expect(language).toHaveFocus();
    await user.selectOptions(language, "zh-CN");
    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
  });

  it("shows loading status while settings are pending", async () => {
    const pending = deferred<{ status: "ok"; data: AppSettingsDto }>();
    vi.mocked(commands.getSettings).mockReturnValue(pending.promise);
    await renderReadyApp();
    const user = userEvent.setup();
    await user.click(screen.getByRole("link", { name: "Settings" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading…");
    pending.resolve({
      status: "ok",
      data: {
        language: "en",
        appearance: "light",
        lastHouseholdId: "hh-1",
      },
    });
    expect(await screen.findByLabelText("Language")).toBeInTheDocument();
  });

  it("shows settings command errors with role=alert", async () => {
    vi.mocked(commands.getSettings).mockResolvedValue({
      status: "error",
      error: {
        code: "DATABASE_UNAVAILABLE",
        message: "The database is unavailable.",
        fields: null,
      },
    });
    await renderReadyApp();
    const user = userEvent.setup();
    await user.click(screen.getByRole("link", { name: "Settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("requires a second keyboard-accessible confirmation before deleting data", async () => {
    const user = userEvent.setup();
    const settings: AppSettingsDto = {
      language: "en",
      appearance: "light",
      lastHouseholdId: "hh-1",
    };
    mockSettings(settings);
    vi.mocked(commands.deleteAllData).mockResolvedValue({ status: "ok", data: null });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Settings" }));

    await user.click(await screen.findByRole("button", { name: "Delete all data" }));
    expect(commands.deleteAllData).not.toHaveBeenCalled();

    const confirm = screen.getByRole("button", {
      name: "Delete everything and restart",
    });
    expect(confirm).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(commands.deleteAllData).toHaveBeenCalledWith({ confirmed: true });
  });

  it("can cancel deletion without invoking the command", async () => {
    const user = userEvent.setup();
    const settings: AppSettingsDto = {
      language: "en",
      appearance: "light",
      lastHouseholdId: "hh-1",
    };
    mockSettings(settings);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Settings" }));
    await user.click(await screen.findByRole("button", { name: "Delete all data" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(commands.deleteAllData).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "Delete everything and restart" }),
    ).not.toBeInTheDocument();
  });
});

function mockSettings(settings: AppSettingsDto) {
  vi.mocked(commands.getSettings).mockImplementation(async () => ({
    status: "ok",
    data: { ...settings },
  }));
  vi.mocked(commands.updateSettings).mockImplementation(async (input) => {
    settings.language = input.language;
    settings.appearance = input.appearance;
    return { status: "ok", data: { ...settings } };
  });
  vi.mocked(commands.bootstrap).mockImplementation(async () => ({
    status: "ok",
    data: {
      ...readyBootstrap,
      settings: { ...settings },
    },
  }));
}
