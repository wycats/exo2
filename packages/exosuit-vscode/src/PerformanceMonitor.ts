import { LogService } from './LogService';

export class PerformanceMonitor {
    private static marks = new Map<string, number>();

    static start(label: string) {
        this.marks.set(label, performance.now());
    }

    static end(label: string) {
        const start = this.marks.get(label);
        if (start) {
            const duration = performance.now() - start;
            // Log if duration is significant (> 10ms) to avoid noise, 
            // but keep it low enough to catch cumulative issues.
            if (duration > 10) { 
                LogService.instance.logActivity({
                    type: 'system',
                    label: `Performance: ${label}`,
                    details: `Duration: ${duration.toFixed(2)}ms`,
                    icon: 'watch'
                });
            }
            this.marks.delete(label);
        }
    }

    static async measure<T>(label: string, fn: () => Promise<T>): Promise<T> {
        this.start(label);
        try {
            return await fn();
        } finally {
            this.end(label);
        }
    }

    static measureSync<T>(label: string, fn: () => T): T {
        this.start(label);
        try {
            return fn();
        } finally {
            this.end(label);
        }
    }
}
