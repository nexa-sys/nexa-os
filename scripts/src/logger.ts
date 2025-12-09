/**
 * NexaOS Build System - Logger
 * Beautiful console output with colors and spinners
 */

import chalk from 'chalk';
import ora, { Ora } from 'ora';

export class Logger {
  private static instance: Logger;
  private currentSpinner: Ora | null = null;
  private startTime: number = Date.now();
  private stepTimes: Map<string, number> = new Map();
  
  static getInstance(): Logger {
    if (!Logger.instance) {
      Logger.instance = new Logger();
    }
    return Logger.instance;
  }
  
  /**
   * Log info message
   */
  info(message: string): void {
    this.stopSpinner();
    console.log(chalk.blue('[INFO]'), message);
  }
  
  /**
   * Log success message
   */
  success(message: string): void {
    this.stopSpinner();
    console.log(chalk.green('[✓]'), message);
  }
  
  /**
   * Log warning message
   */
  warn(message: string): void {
    this.stopSpinner();
    console.log(chalk.yellow('[WARN]'), message);
  }
  
  /**
   * Log error message
   */
  error(message: string): void {
    this.stopSpinner();
    console.error(chalk.red('[ERROR]'), message);
  }
  
  /**
   * Log a build step
   */
  step(message: string): void {
    this.stopSpinner();
    console.log(chalk.cyan('==>'), message);
  }
  
  /**
   * Log a section header
   */
  section(title: string): void {
    this.stopSpinner();
    const border = '='.repeat(40);
    console.log('');
    console.log(chalk.cyan(border));
    console.log(chalk.cyan(title));
    console.log(chalk.cyan(border));
    console.log('');
  }
  
  /**
   * Start a spinner for long-running operations
   */
  startSpinner(text: string): Ora {
    this.stopSpinner();
    this.currentSpinner = ora({
      text,
      color: 'cyan',
    }).start();
    return this.currentSpinner;
  }
  
  /**
   * Stop current spinner
   */
  stopSpinner(): void {
    if (this.currentSpinner) {
      this.currentSpinner.stop();
      this.currentSpinner = null;
    }
  }
  
  /**
   * Complete spinner with success
   */
  spinnerSuccess(text?: string): void {
    if (this.currentSpinner) {
      this.currentSpinner.succeed(text);
      this.currentSpinner = null;
    }
  }
  
  /**
   * Complete spinner with failure
   */
  spinnerFail(text?: string): void {
    if (this.currentSpinner) {
      this.currentSpinner.fail(text);
      this.currentSpinner = null;
    }
  }
  
  /**
   * Update spinner text
   */
  spinnerUpdate(text: string): void {
    if (this.currentSpinner) {
      this.currentSpinner.text = text;
    }
  }
  
  /**
   * Start timing a step
   */
  startTimer(name: string): void {
    this.stepTimes.set(name, Date.now());
  }
  
  /**
   * Get elapsed time for a step
   */
  getElapsed(name: string): number {
    const start = this.stepTimes.get(name);
    if (!start) return 0;
    return (Date.now() - start) / 1000;
  }
  
  /**
   * Log step completion with timing
   */
  stepComplete(name: string, message?: string): void {
    const elapsed = this.getElapsed(name);
    const text = message ?? name;
    this.success(`${text} (${elapsed.toFixed(1)}s)`);
    this.stepTimes.delete(name);
  }
  
  /**
   * Reset the global timer
   */
  resetTimer(): void {
    this.startTime = Date.now();
    this.stepTimes.clear();
  }
  
  /**
   * Get total elapsed time
   */
  getTotalTime(): number {
    return (Date.now() - this.startTime) / 1000;
  }
  
  /**
   * Log build summary
   */
  summary(artifacts: { name: string; path: string; size?: string }[]): void {
    const totalTime = this.getTotalTime();
    
    console.log('');
    this.section(`Build Complete! (${totalTime.toFixed(1)}s)`);
    
    console.log('System components:');
    for (const artifact of artifacts) {
      const sizeInfo = artifact.size ? ` (${artifact.size})` : '';
      console.log(`  - ${artifact.name}: ${artifact.path}${sizeInfo}`);
    }
    
    console.log('');
    console.log('To run in QEMU:');
    console.log(' ./ndk run');
    console.log('');
  }
  
  /**
   * Create a boxed message
   */
  box(title: string, content: string[]): void {
    const maxLen = Math.max(title.length, ...content.map(l => l.length));
    const border = '─'.repeat(maxLen + 2);
    
    console.log(chalk.gray(`┌${border}┐`));
    console.log(chalk.gray('│'), chalk.bold(title.padEnd(maxLen)), chalk.gray('│'));
    console.log(chalk.gray(`├${border}┤`));
    for (const line of content) {
      console.log(chalk.gray('│'), line.padEnd(maxLen), chalk.gray('│'));
    }
    console.log(chalk.gray(`└${border}┘`));
  }
  
  /**
   * Log a table
   */
  table(headers: string[], rows: string[][]): void {
    const widths = headers.map((h, i) => 
      Math.max(h.length, ...rows.map(r => r[i]?.length ?? 0))
    );
    
    const separator = widths.map(w => '─'.repeat(w + 2)).join('┼');
    
    // Header
    console.log(chalk.gray('┌' + widths.map(w => '─'.repeat(w + 2)).join('┬') + '┐'));
    console.log(
      chalk.gray('│') + 
      headers.map((h, i) => ` ${chalk.bold(h.padEnd(widths[i]))} `).join(chalk.gray('│')) +
      chalk.gray('│')
    );
    console.log(chalk.gray('├' + separator + '┤'));
    
    // Rows
    for (const row of rows) {
      console.log(
        chalk.gray('│') + 
        row.map((cell, i) => ` ${(cell ?? '').padEnd(widths[i])} `).join(chalk.gray('│')) +
        chalk.gray('│')
      );
    }
    
    console.log(chalk.gray('└' + widths.map(w => '─'.repeat(w + 2)).join('┴') + '┘'));
  }
  
  /**
   * Progress bar (simple text-based)
   */
  progress(current: number, total: number, label: string): void {
    const width = 30;
    const percent = Math.floor((current / total) * 100);
    const filled = Math.floor((current / total) * width);
    const empty = width - filled;
    
    const bar = chalk.green('█'.repeat(filled)) + chalk.gray('░'.repeat(empty));
    
    process.stdout.write(`\r${bar} ${percent.toString().padStart(3)}% ${label}`);
    
    if (current === total) {
      console.log('');
    }
  }
}

// Export singleton
export const logger = Logger.getInstance();
