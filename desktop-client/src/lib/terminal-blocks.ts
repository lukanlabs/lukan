import type { Terminal } from "@xterm/xterm";

export interface TerminalBlock {
  /** 0-based start line in the scrollback buffer. */
  startLine: number;
  /** 0-based end line (inclusive) — updated as output flows. */
  endLine: number;
  /** The prompt / command text (best-effort extraction). */
  command: string;
  /** Timestamp of when the block was detected. */
  timestamp: number;
}

/**
 * Detects command blocks in the terminal buffer by matching prompt patterns.
 *
 * Warp-style: each command + its output forms a "block" visually separated
 * from the next. We detect block boundaries by matching common shell prompt
 * patterns at the start of lines.
 */
export class BlockDetector {
  private blocks: TerminalBlock[] = [];
  private lastCheckedLine = 0;

  /** Common prompt patterns — `$`, `%`, `#`, `❯`, `➜`, or user@host:path$. */
  private static PROMPT_RE =
    /^(?:.*?[@:].*?[\$#%>❯➜]|[\$#%>❯➜])\s/;

  constructor(private terminal: Terminal) {}

  /** Scan new lines since last check and return updated block list. */
  scan(): TerminalBlock[] {
    const buffer = this.terminal.buffer.active;
    const totalLines = buffer.baseY + buffer.cursorY + 1;

    for (let i = this.lastCheckedLine; i < totalLines; i++) {
      const line = buffer.getLine(i);
      if (!line) continue;

      const text = line.translateToString(true);
      if (BlockDetector.PROMPT_RE.test(text)) {
        // Close previous block
        if (this.blocks.length > 0) {
          this.blocks[this.blocks.length - 1].endLine = i - 1;
        }
        // Open new block
        this.blocks.push({
          startLine: i,
          endLine: i,
          command: text.replace(BlockDetector.PROMPT_RE, "").trim(),
          timestamp: Date.now(),
        });
      } else if (this.blocks.length > 0) {
        // Extend current block
        this.blocks[this.blocks.length - 1].endLine = i;
      }
    }

    this.lastCheckedLine = totalLines;
    return this.blocks;
  }

  /** Get the current list of detected blocks. */
  getBlocks(): TerminalBlock[] {
    return this.blocks;
  }

  /** Reset detection state. */
  reset(): void {
    this.blocks = [];
    this.lastCheckedLine = 0;
  }
}
