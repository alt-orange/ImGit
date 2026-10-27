// Runs before ImGit's main() in the browser build (--pre-js in CMakeLists.txt).
//
// The home folder (/home/web_user: the settings, recent repositories, avatar cache and the repositories made
// here) lives in the browser's IndexedDB: loaded before main() starts, written back by Module.imgitSyncFiles,
// which src/platform/main_loop_web.cpp calls every few seconds and when the page is hidden.
if (typeof ENVIRONMENT_IS_PTHREAD === 'undefined' || !ENVIRONMENT_IS_PTHREAD) {
    Module.preRun = Module.preRun || [];
    Module.preRun.push(function () {
        var home = '/home/web_user';
        FS.mkdirTree(home);
        FS.mount(IDBFS, {}, home);
        addRunDependency('imgit-home');
        FS.syncfs(true, function (error) {
            if (error) {
                console.error('ImGit: the stored files could not be read', error);
            }
            removeRunDependency('imgit-home');
        });
    });

    // One write-back at a time (IndexedDB transactions must not overlap); calls made meanwhile run once after it,
    // and `done` (optional) runs when the files written by then are stored.
    Module.imgitSyncFiles = (function () {
        var running = false;
        var waiting = [];
        function run(callbacks) {
            running = true;
            FS.syncfs(false, function (error) {
                if (error) {
                    console.error('ImGit: the files could not be stored', error);
                }
                callbacks.forEach(function (callback) {
                    callback();
                });
                running = false;
                if (waiting.length > 0) {
                    var next = waiting;
                    waiting = [];
                    run(next.filter(Boolean));
                }
            });
        }
        return function (done) {
            if (running) {
                waiting.push(done || null);
                return;
            }
            run(done ? [done] : []);
        };
    })();
}
