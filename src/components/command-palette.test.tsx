import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { renderReadyApp, resetApp } from "@/test/app-harness";
describe("command palette", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("opens with the accessible trigger and restores focus on Escape", async () => {
    await renderReadyApp();
    const user = userEvent.setup();
    const trigger = screen.getByRole("button", { name: "Open command palette" });
    await user.click(trigger);
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    const input = screen.getByRole("combobox", { name: "Command palette" });
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "Command palette" }),
    ).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
    expect(input).not.toBeInTheDocument();
  });
});
