import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App";
import type { BootstrapDto, CommandError } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";

vi.mock("@/generated/tauri-bindings", () => ({
  commands: {
    bootstrap: vi.fn(),
    completeOnboarding: vi.fn(),
  },
}));

const emptyBootstrap: BootstrapDto = {
  status: "ready",
  onboardingRequired: true,
  settings: { language: "system", appearance: "system", lastHouseholdId: null },
  household: null,
  members: [],
};

const readyBootstrap: BootstrapDto = {
  status: "ready",
  onboardingRequired: false,
  settings: { language: "system", appearance: "system", lastHouseholdId: "hh-1" },
  household: { id: "hh-1", name: "Wang Family", baseCurrency: "CNY" },
  members: [
    { id: "m-1", name: "Walt" },
    { id: "m-2", name: "Spouse" },
  ],
};

const blockedBootstrap: BootstrapDto = {
  status: "blocked",
  error: {
    code: "UNSUPPORTED_NEWER_DATABASE",
    message: "This database was created by a newer version of Nestworth.",
    fields: { foundMigration: "999", supportedMigration: "1" },
  },
  databasePath: "/tmp/nestworth.sqlite3",
  foundMigration: 999,
  supportedMigration: 1,
};

function mockBootstrap(data: BootstrapDto) {
  vi.mocked(commands.bootstrap).mockResolvedValue({ status: "ok", data });
}

async function renderApp() {
  render(<App />);
}

describe("startup routing", () => {
  beforeEach(() => {
    vi.mocked(commands.bootstrap).mockReset();
    vi.mocked(commands.completeOnboarding).mockReset();
    window.history.replaceState(null, "", "/");
  });

  it("sends an empty bootstrap into onboarding", async () => {
    mockBootstrap(emptyBootstrap);
    await renderApp();
    expect(
      await screen.findByRole("heading", { name: "Set up your household" }),
    ).toBeInTheDocument();
  });

  it("sends an existing household into overview", async () => {
    mockBootstrap(readyBootstrap);
    await renderApp();
    expect(
      await screen.findByRole("heading", { name: "Wang Family" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No accounts yet. You can add them later."),
    ).toBeInTheDocument();
  });

  it("sends a blocked bootstrap into the startup error page", async () => {
    mockBootstrap(blockedBootstrap);
    await renderApp();
    expect(
      await screen.findByRole("heading", {
        name: "Nestworth cannot open this database",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("This database is at migration 999, but this app supports 1."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Finish" })).not.toBeInTheDocument();
  });
});

describe("onboarding flow", () => {
  beforeEach(() => {
    vi.mocked(commands.bootstrap).mockReset();
    vi.mocked(commands.completeOnboarding).mockReset();
    window.history.replaceState(null, "", "/");
    mockBootstrap(emptyBootstrap);
  });

  it("keeps values when moving back and forward", async () => {
    const user = userEvent.setup();
    await renderApp();
    await screen.findByRole("heading", { name: "Set up your household" });

    await user.type(screen.getByLabelText("Household name"), "Wang Family");
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByLabelText("Household name")).toHaveValue("Wang Family");
  });

  it("focuses the first invalid field on validation failure", async () => {
    const user = userEvent.setup();
    await renderApp();
    await screen.findByRole("heading", { name: "Set up your household" });
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByLabelText("Household name")).toHaveFocus();
    expect(screen.getByText("This field is required.")).toBeInTheDocument();
  });

  it("completes onboarding, refetches bootstrap, and opens overview", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.completeOnboarding).mockImplementation(async () => {
      mockBootstrap(readyBootstrap);
      return { status: "ok", data: null };
    });
    await renderApp();
    await completeValidOnboarding(user);

    expect(commands.completeOnboarding).toHaveBeenCalledTimes(1);
    expect(commands.completeOnboarding).toHaveBeenCalledWith({
      householdName: "Wang Family",
      baseCurrency: "CNY",
      members: [{ name: "Walt" }],
    });
    expect(
      await screen.findByRole("heading", { name: "Wang Family" }),
    ).toBeInTheDocument();
  });

  it("shows a safe ALREADY_ONBOARDED error", async () => {
    const user = userEvent.setup();
    const error: CommandError = {
      code: "ALREADY_ONBOARDED",
      message: "This household has already been set up.",
      fields: null,
    };
    vi.mocked(commands.completeOnboarding).mockResolvedValue({
      status: "error",
      error,
    });
    await renderApp();
    await completeValidOnboarding(user);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This household has already been set up.",
    );
    expect(
      screen.queryByRole("heading", { name: "Wang Family" }),
    ).not.toBeInTheDocument();
  });

  it("does not submit twice while the mutation is pending", async () => {
    const user = userEvent.setup();
    let finish: ((value: { status: "ok"; data: null }) => void) | undefined;
    vi.mocked(commands.completeOnboarding).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await renderApp();
    await completeValidOnboarding(user);
    const finishButton = await screen.findByRole("button", { name: "Saving" });
    expect(finishButton).toBeDisabled();
    await user.click(finishButton);
    expect(commands.completeOnboarding).toHaveBeenCalledTimes(1);
    finish?.({ status: "ok", data: null });
  });

  it("can complete onboarding with the keyboard", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.completeOnboarding).mockImplementation(async () => {
      mockBootstrap(readyBootstrap);
      return { status: "ok", data: null };
    });
    await renderApp();
    await screen.findByRole("heading", { name: "Set up your household" });
    expect(screen.getByLabelText("Household name")).toHaveFocus();
    await user.keyboard("Wang Family{Enter}");
    await screen.findByText("Base currency");
    await user.keyboard("{Enter}");
    const member = await screen.findByLabelText("Member 1");
    await user.type(member, "Walt");
    await user.keyboard("{Enter}");
    await screen.findByRole("heading", { name: "Review" });
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "Wang Family" }),
    ).toBeInTheDocument();
  });

  it("cannot delete the last member", async () => {
    const user = userEvent.setup();
    await renderApp();
    await screen.findByRole("heading", { name: "Set up your household" });
    await user.type(screen.getByLabelText("Household name"), "Wang Family");
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Next" }));
    const remove = screen.getByRole("button", { name: "Remove member 1" });
    expect(remove).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Add member" }));
    expect(screen.getByLabelText("Member 2")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove member 2" }));
    expect(screen.queryByLabelText("Member 2")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove member 1" })).toBeDisabled();
  });
});

async function completeValidOnboarding(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByRole("heading", { name: "Set up your household" });
  await user.type(screen.getByLabelText("Household name"), "Wang Family");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.type(screen.getByLabelText("Member 1"), "Walt");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Finish" }));
}
