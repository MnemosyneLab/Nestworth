import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

import { queryClient } from "@/app/query-client";
import { i18n } from "@/lib/i18n";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("@/lib/tauri/pick-image", () => ({
  pickImagePath: vi.fn(),
}));

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
      listAccounts: vi.fn(),
      getAccount: vi.fn(),
      createAccount: vi.fn(),
      updateAccount: vi.fn(),
      updateAccountValue: vi.fn(),
      archiveAccount: vi.fn(),
      restoreAccount: vi.fn(),
      getOverview: vi.fn(),
      setMemberAvatar: vi.fn(),
      setInstitutionLogo: vi.fn(),
      setGroupLogo: vi.fn(),
      setAccountLogo: vi.fn(),
      getMedia: vi.fn(),
      getSettings: vi.fn(),
      updateSettings: vi.fn(),
    },
  };
});

Object.defineProperty(window, "scrollTo", {
  configurable: true,
  value: () => undefined,
});

Object.defineProperty(window, "matchMedia", {
  configurable: true,
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  }),
});

afterEach(() => {
  queryClient.clear();
  document.documentElement.classList.remove("dark");
  void i18n.changeLanguage("en");
  cleanup();
});
