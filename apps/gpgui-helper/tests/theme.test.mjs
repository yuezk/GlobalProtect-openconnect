import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { test } from "node:test";
import assert from "node:assert/strict";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];

function load(theme, dark) {
  const root = { dataset: {} };
  let changed;
  const media = {
    matches: dark,
    addEventListener(event, listener) {
      assert.equal(event, "change");
      changed = listener;
    },
  };
  runInNewContext(script, {
    window: { __GP_HELPER_THEME__: theme },
    document: { documentElement: root },
    matchMedia(query) {
      assert.equal(query, "(prefers-color-scheme: dark)");
      return media;
    },
  });
  return { root, change(dark) { media.matches = dark; changed?.(); } };
}

test("default and explicit system modes follow system changes before React starts", () => {
  for (const theme of [undefined, "system"]) {
    for (const dark of [false, true]) {
      const page = load(theme, dark);
      assert.equal(page.root.dataset.theme, dark ? "dark" : "light");
      page.change(!dark);
      assert.equal(page.root.dataset.theme, dark ? "light" : "dark");
    }
  }
});

test("explicit modes override opposite system appearance and ignore system changes", () => {
  for (const theme of ["light", "dark"]) {
    const page = load(theme, theme !== "dark");
    assert.equal(page.root.dataset.theme, theme);
    page.change(theme === "dark");
    assert.equal(page.root.dataset.theme, theme);
  }
});
