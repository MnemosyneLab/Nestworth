import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

import { queryClient } from "@/app/query-client";

vi.mock("@/generated/tauri-bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/generated/tauri-bindings")>();
  return {
    ...actual,
    commands: {
      bootstrap: vi.fn(),
      completeOnboarding: vi.fn(),
      listMembers: vi.fn(),
      createMember: vi.fn(),
      updateMember: vi.fn(),
      archiveMember: vi.fn(),
      restoreMember: vi.fn(),
      listInstitutions: vi.fn(),
      createInstitution: vi.fn(),
      updateInstitution: vi.fn(),
      archiveInstitution: vi.fn(),
      restoreInstitution: vi.fn(),
      listGroups: vi.fn(),
      createGroup: vi.fn(),
      updateGroup: vi.fn(),
      archiveGroup: vi.fn(),
      restoreGroup: vi.fn(),
    },
  };
});

Object.defineProperty(window, "scrollTo", {
  configurable: true,
  value: () => undefined,
});

afterEach(() => {
  queryClient.clear();
  cleanup();
});
