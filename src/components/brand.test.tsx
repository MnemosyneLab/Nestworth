import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Brand } from "@/components/brand";

describe("Brand", () => {
  it("exposes the wordmark as the accessible brand name in a lockup", () => {
    const { container, getByRole } = render(<Brand />);

    expect(getByRole("img", { name: "Nestworth" })).toHaveAttribute(
      "src",
      "/brand/wordmark.png",
    );
    const mark = container.querySelector('img[src="/brand/logo-mark.png"]');
    expect(mark).toHaveAttribute("alt", "");
    expect(mark).toHaveAttribute("aria-hidden", "true");
  });

  it("keeps a standalone mark accessible", () => {
    const { getByRole } = render(<Brand size="sm" variant="mark" />);

    expect(getByRole("img", { name: "Nestworth" })).toHaveAttribute(
      "src",
      "/brand/logo-mark.png",
    );
  });
});
