import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const ROOT = dirname(fileURLToPath(import.meta.url));

const FORBIDDEN = [
  { name: "parseFloat", pattern: /parseFloat\s*\(/ },
  { name: "Number(", pattern: /Number\s*\(/ },
  { name: "* 365", pattern: /\*\s*365\b/ },
  { name: "/ 365", pattern: /\/\s*365\b/ },
  { name: "Math.pow", pattern: /Math\.pow\s*\(/ },
  { name: "invalidateValuation", pattern: /invalidateValuation/ },
];

describe("analytics frontend source contract", () => {
  it("does not compute gain, rate, annualization, or invalidate valuation", () => {
    const files = listSourceFiles(ROOT).filter(
      (path) => !path.endsWith(".test.ts") && !path.endsWith(".test.tsx"),
    );
    expect(files.length).toBeGreaterThan(0);
    const violations: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, "utf8");
      for (const rule of FORBIDDEN) {
        if (rule.pattern.test(source)) {
          violations.push(`${file}: ${rule.name}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

function listSourceFiles(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      return listSourceFiles(path);
    }
    return path.endsWith(".ts") || path.endsWith(".tsx") ? [path] : [];
  });
}
