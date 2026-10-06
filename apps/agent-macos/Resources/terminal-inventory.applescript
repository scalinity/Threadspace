-- Read-only Terminal inventory: one "<window id><TAB><tty>" line per tab.
-- Fixed bundled script run by the companion's bounded osascript worker. It
-- never selects, focuses, activates or types (SPEC §13.3). The companion only
-- runs it while Terminal is already running.
on run argv
	-- Bound outside the tell block: inside it, `tab` names Terminal's tab class.
	set separator to character id 9
	set rows to {}
	tell application "Terminal"
		repeat with aWindow in windows
			set windowId to id of aWindow
			repeat with aTab in tabs of aWindow
				set end of rows to ((windowId as text) & separator & (tty of aTab))
			end repeat
		end repeat
	end tell
	set AppleScript's text item delimiters to linefeed
	set output to rows as text
	set AppleScript's text item delimiters to ""
	return output
end run
