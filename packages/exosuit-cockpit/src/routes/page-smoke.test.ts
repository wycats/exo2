import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";

import Page from "./+page.svelte";

describe("cockpit package shell", () => {
  it("renders the dormant lane workbench placeholder", () => {
    render(Page);

    expect(
      screen.getByRole("heading", { name: "Lane-centered workbench pending" }),
    ).toBeTruthy();
  });
});
