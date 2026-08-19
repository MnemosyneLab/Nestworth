import { describe, expect, it } from "vitest";

import { barGeometry } from "@/features/analytics/model";

describe("analytics chart geometry", () => {
  it("scales coordinates without changing the DTO amount strings", () => {
    const amounts = ["160.0000", "80.0000", "100.0000", "-200.0000"];
    const geometry = barGeometry(amounts);
    expect(geometry.bars).toHaveLength(4);
    expect(geometry.viewBox).toBe("0 0 640 220");
    expect(amounts).toEqual(["160.0000", "80.0000", "100.0000", "-200.0000"]);
  });
});
