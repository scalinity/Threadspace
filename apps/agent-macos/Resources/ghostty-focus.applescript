// Fixed JXA resource, run with osascript -l JavaScript. Native ID focus only.
// argv: exact application PID, window/tab/terminal IDs, settle budget (ms),
// frontmost capability, max windows, max tabs, max terminals.
// No input, keypress, title/cwd matching or application-only fallback.
// FOCUSED <target window/tab/terminal> <front/selected/focused IDs>
//         <target tab selected> <app frontmost or -> <frontmost app PID>.
ObjC.import('Foundation');
ObjC.import('AppKit');

function uniqueIds(ids) {
    return new Set(ids).size === ids.length;
}

function sameIds(a, b) {
    return a.length === b.length && a.every((id, index) => id === b[index]);
}

function nativeId(object) {
    const id = object.id();
    if (typeof id !== 'string' || !id.length || id.length > 256 || /[\x00-\x1f\x7f]/.test(id)) {
        throw Error('Invalid Ghostty native ID');
    }
    return id;
}

function idsOf(objects, maximum) {
    if (objects.length > maximum) throw Error('Ghostty inventory exceeds limit');
    return objects.map(nativeId);
}

function run(argv) {
    if (argv.length !== 9) throw Error('Ghostty focus arguments');
    const appPid = Number(argv[0]);
    if (!Number.isInteger(appPid) || appPid <= 0 || appPid > 2147483647) throw Error('Invalid application PID');
    const targetWindowId = argv[1];
    const targetTabId = argv[2];
    const targetTerminalId = argv[3];
    const budgetMs = Number(argv[4]);
    const canReadFrontmost = argv[5] === 'true';
    const maxWindows = Number(argv[6]);
    const maxTabs = Number(argv[7]);
    const maxTerminals = Number(argv[8]);
    const app = Application(appPid);
    if (!app.running()) return 'GONE';
    const windowIds = idsOf(app.windows(), maxWindows);
    const appTerminalIds = idsOf(app.terminals(), maxTerminals);
    if (!uniqueIds(windowIds) || !uniqueIds(appTerminalIds)) return 'CHANGED';
    const seenTabs = [];
    const seenTerminals = [];
    const windows = [];
    const tabs = [];
    const matches = [];
    windowIds.forEach(windowId => {
        const window = app.windows.byId(windowId);
        const tabIds = idsOf(window.tabs(), maxTabs - seenTabs.length);
        windows.push({ id: windowId, tabIds });
        tabIds.forEach(tabId => {
            seenTabs.push(tabId);
            const terminalIds = idsOf(window.tabs.byId(tabId).terminals(), maxTerminals - seenTerminals.length);
            tabs.push({ windowId, id: tabId, terminalIds });
            terminalIds.forEach(terminalId => {
                seenTerminals.push(terminalId);
                if (terminalId === targetTerminalId) matches.push({ windowId, tabId });
            });
        });
    });
    if (!matches.length) return 'GONE';
    if (matches.length > 1) return ['AMBIGUOUS', matches.length].join('\t');
    if (!uniqueIds(seenTabs) || !uniqueIds(seenTerminals) ||
        !sameIds(idsOf(app.windows(), maxWindows), windowIds) ||
        !sameIds(idsOf(app.terminals(), maxTerminals), appTerminalIds) ||
        seenTerminals.length !== appTerminalIds.length ||
        !appTerminalIds.every(id => seenTerminals.includes(id))) return 'CHANGED';
    if (!windows.every(snapshot => sameIds(idsOf(app.windows.byId(snapshot.id).tabs(), maxTabs), snapshot.tabIds)) ||
        !tabs.every(snapshot => sameIds(idsOf(app.windows.byId(snapshot.windowId).tabs.byId(snapshot.id).terminals(), maxTerminals), snapshot.terminalIds))) return 'CHANGED';
    if (matches[0].windowId !== targetWindowId || matches[0].tabId !== targetTabId) return 'CHANGED';

    const targetWindow = app.windows.byId(targetWindowId);
    const targetTab = targetWindow.tabs.byId(targetTabId);
    const targetTerminal = targetTab.terminals.byId(targetTerminalId);
    if (nativeId(targetWindow) !== targetWindowId || nativeId(targetTab) !== targetTabId ||
        nativeId(targetTerminal) !== targetTerminalId) return 'CHANGED';
    // Native focus selects the pane but does not itself activate Ghostty.
    // Activate this exact PID, then select and verify the exact native triple.
    app.activate();
    app.focus(targetTerminal);

    // Native Space/window transitions may settle asynchronously. The bounded
    // subprocess enforces the full budget, including every native query.
    let readback;
    for (let remaining = Math.floor(budgetMs / 25); ; remaining--) {
        const frontWindow = app.frontWindow();
        const selectedTab = frontWindow.selectedTab();
        readback = {
            front: nativeId(frontWindow), selected: nativeId(selectedTab),
            focused: nativeId(selectedTab.focusedTerminal()), targetSelected: targetTab.selected(),
            frontmost: canReadFrontmost ? app.frontmost() : '-',
            frontmostPid: Number($.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier)
        };
        if ((readback.front === targetWindowId && readback.selected === targetTabId &&
             readback.focused === targetTerminalId && readback.targetSelected === true &&
             (!canReadFrontmost || readback.frontmost === true) &&
             readback.frontmostPid === appPid) || remaining <= 0) break;
        $.NSThread.sleepForTimeInterval(0.025);
    }
    return ['FOCUSED', nativeId(targetWindow), nativeId(targetTab), nativeId(targetTerminal),
        readback.front, readback.selected, readback.focused, readback.targetSelected,
        readback.frontmost, readback.frontmostPid].join('\t');
}
