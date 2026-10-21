import AppKit

@MainActor final class AtelierTray: NSObject, NSApplicationDelegate {
    let root = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().deletingLastPathComponent().deletingLastPathComponent()
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    let menu = NSMenu()
    var busy = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let image = NSImage(contentsOf: root.appendingPathComponent("public/atelier-kanban.png"))
        image?.size = NSSize(width: 18, height: 18)
        item.button?.image = image
        item.button?.toolTip = "Atelier Kanban"
        menu.autoenablesItems = false
        for (title, action) in [("Ouvrir Atelier", #selector(openAtelier)), ("Quitter", #selector(quitAtelier))] {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.target = self
            menu.addItem(entry)
        }
        item.menu = menu
        // The pipe belongs to the server, including across updates and crashes.
        FileHandle.standardInput.readabilityHandler = { input in
            if input.availableData.isEmpty {
                input.readabilityHandler = nil
                DispatchQueue.main.async { NSApplication.shared.terminate(nil) }
            }
        }
    }

    @objc func openAtelier() { launch([]) }
    @objc func quitAtelier() { launch(["--stop"]) }

    func launch(_ arguments: [String]) {
        if busy { return }
        busy = true
        menu.items.forEach { $0.isEnabled = false }
        let root = self.root
        DispatchQueue.global().async {
            var message = ""
            do {
                let process = Process()
                process.executableURL = root.appendingPathComponent("runtime/node")
                process.arguments = [root.appendingPathComponent("launcher.mjs").path] + arguments
                process.currentDirectoryURL = root
                process.standardInput = FileHandle.nullDevice
                process.standardOutput = FileHandle.nullDevice
                let errors = Pipe()
                process.standardError = errors
                try process.run()
                let output = errors.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                if process.terminationStatus != 0 {
                    message = String(data: output, encoding: .utf8) ?? ""
                    if message.isEmpty { message = "Le lanceur Atelier a échoué." }
                }
            } catch { message = error.localizedDescription }
            let result = message
            DispatchQueue.main.async {
                self.busy = false
                self.menu.items.forEach { $0.isEnabled = true }
                if !result.isEmpty {
                    let alert = NSAlert()
                    alert.messageText = "Atelier Kanban"
                    alert.informativeText = result
                    alert.alertStyle = .warning
                    alert.runModal()
                }
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        FileHandle.standardInput.readabilityHandler = nil
        NSStatusBar.system.removeStatusItem(item)
    }
}

let application = NSApplication.shared
let delegate = AtelierTray()
application.setActivationPolicy(.accessory)
application.delegate = delegate
application.run()
