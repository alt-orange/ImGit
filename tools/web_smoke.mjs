// Smoke test of the browser build: opens imgit.html in headless Chromium (WebGL 2 through SwiftShader), waits for
// the app to run its frames and saves a screenshot. Fails on an uncaught error, an abort or a status line that says
// the page stopped.
//
//   node tools/web_smoke.mjs <url of imgit.html> <screenshot.png>
//
// Needs the playwright package; the page must be served cross-origin isolated (tools/serve_web.py).
import { chromium } from 'playwright';

const [url, screenshot] = process.argv.slice(2);
if (!url || !screenshot) {
    console.error('usage: node tools/web_smoke.mjs <url> <screenshot.png>');
    process.exit(2);
}

const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on('pageerror', (error) => errors.push(`page error: ${error.message}`));
page.on('console', (message) => {
    console.log(`[${message.type()}] ${message.text()}`);
    if (message.type() === 'error' && /abort|Aborted|RuntimeError/i.test(message.text())) {
        errors.push(`console: ${message.text()}`);
    }
});

let exitCode = 0;
try {
    await page.goto(url);
    if (!(await page.evaluate(() => window.crossOriginIsolated))) {
        throw new Error('the page is not cross-origin isolated (missing COOP / COEP headers)');
    }
    // onRuntimeInitialized hides the status line once main() is about to run.
    await page.waitForSelector('#status', { state: 'hidden', timeout: 60000 });
    // A few seconds of frames: the first repository-less screen, the fonts baked, the IndexedDB write-back.
    await page.waitForTimeout(5000);
    await page.screenshot({ path: screenshot });
    const status = await page.evaluate(() => {
        const line = document.getElementById('status');
        return line.style.display === 'none' ? '' : line.textContent;
    });
    if (status) {
        errors.push(`status: ${status}`);
    }
} catch (error) {
    errors.push(error.message);
    await page.screenshot({ path: screenshot }).catch(() => {});
} finally {
    await browser.close();
}
if (errors.length > 0) {
    console.error(errors.join('\n'));
    exitCode = 1;
}
process.exit(exitCode);
