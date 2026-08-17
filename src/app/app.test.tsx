import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "@/App";

describe("application foundation", () => {
  it("renders the routed, translated foundation screen", async () => {
    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Nestworth" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Foundation ready" }),
    ).toBeInTheDocument();
  });
});
