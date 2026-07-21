import { describe, expect, it } from "vitest";
import type { ConversationDocument } from "./types";
import { bundleDocuments, conversationTimestamp, groupByDay, modelLabel, searchTerms, shortPath, toggleResultSelection } from "./App";

function document(id: number, startedAt: string, markdown: string): ConversationDocument {
  return {
    id,
    source: "codex",
    external_id: `session-${id}`,
    title: `Session ${id}`,
    cwd: null,
    started_via: "Codex Desktop",
    start_command: null,
    models: ["gpt-5.6-sol"],
    started_at: startedAt,
    updated_at: startedAt,
    approx_clean_tokens: 10,
    approx_full_tokens: 20,
    message_count: 2,
    user_message_count: 1,
    assistant_message_count: 1,
    clean_markdown_path: `/tmp/${id}.md`,
    full_markdown_path: `/tmp/${id}.full.md`,
    source_path: `/tmp/${id}.jsonl`,
    markdown,
  };
}

describe("conversation ordering dates", () => {
  it("switches timestamps and day groups between start and last message", () => {
    const conversation = {
      ...document(1, "2026-07-01T10:00:00Z", "# One\n"),
      updated_at: "2026-07-03T12:00:00Z",
    };

    expect(conversationTimestamp(conversation, "started_at")).toBe("2026-07-01T10:00:00Z");
    expect(conversationTimestamp(conversation, "updated_at")).toBe("2026-07-03T12:00:00Z");
    expect(groupByDay([conversation], "started_at")[0][0]).toBe("2026-07-01");
    expect(groupByDay([conversation], "updated_at")[0][0]).toBe("2026-07-03");
  });
});

describe("modelLabel", () => {
  it("preserves the order of model switches", () => {
    expect(modelLabel(["gpt-5.5", "gpt-5.6"])).toBe("gpt-5.5 → gpt-5.6");
  });
});

describe("searchTerms", () => {
  it("keeps phrases, removes operators, and deduplicates case-insensitively", () => {
    expect(searchTerms('"deployment failure" AND codex or CODEX')).toEqual([
      "deployment failure",
      "codex",
    ]);
  });

  it("escapes punctuation into useful search tokens", () => {
    expect(searchTerms("foo/bar near app-kit")).toEqual(["app-kit", "foo", "bar"]);
  });
});

describe("shortPath", () => {
  it("uses the discovered home directory instead of a developer-specific path", () => {
    expect(shortPath("/Users/alex/code/project", "/Users/alex")).toBe("~/code/project");
    expect(shortPath("/Volumes/work/project", "/Users/alex")).toBe("/Volumes/work/project");
  });
});

describe("toggleResultSelection", () => {
  it("selects and deselects every visible result without clearing hidden selections", () => {
    const hidden = document(3, "2026-07-03T10:00:00Z", "# Three\n");
    const results = [
      document(1, "2026-07-01T10:00:00Z", "# One\n"),
      document(2, "2026-07-02T10:00:00Z", "# Two\n"),
    ];

    const selected = toggleResultSelection({ [hidden.id]: hidden }, results);
    expect(Object.keys(selected).map(Number).sort()).toEqual([1, 2, 3]);

    const deselected = toggleResultSelection(selected, results);
    expect(Object.keys(deselected).map(Number)).toEqual([3]);
  });
});

describe("bundleDocuments", () => {
  it("returns a single transcript unchanged", () => {
    expect(bundleDocuments([document(1, "2026-07-01T10:00:00Z", "# One\n")], "clean"))
      .toBe("# One\n");
  });

  it("labels and separates multiple transcripts", () => {
    const bundle = bundleDocuments([
      document(1, "2026-07-01T10:00:00Z", "# One\n"),
      document(2, "2026-07-02T10:00:00Z", "# Two\n"),
    ], "full");
    expect(bundle).toContain("Sessions: 2\nMode: full");
    expect(bundle).toContain("# One\n\n---\n\n# Two");
  });
});
