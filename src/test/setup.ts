import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

import { queryClient } from "@/app/query-client";

Object.defineProperty(window, "scrollTo", {
  configurable: true,
  value: () => undefined,
});

afterEach(() => {
  queryClient.clear();
  cleanup();
});
