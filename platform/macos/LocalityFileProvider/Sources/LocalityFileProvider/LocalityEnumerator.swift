import FileProvider
import Foundation

final class LocalityEnumerator: NSObject, NSFileProviderEnumerator {
    private let client: LocalityDaemonClient?
    private let mountId: String?
    private let containerIdentifier: String?
    private let domainId: String?
    private let namespaceMountId: String?

    init(
        client: LocalityDaemonClient,
        mountId: String,
        containerIdentifier: String,
        namespaceMountId: String? = nil
    ) {
        self.client = client
        self.mountId = mountId
        self.containerIdentifier = containerIdentifier
        self.domainId = nil
        self.namespaceMountId = namespaceMountId
        super.init()
    }

    init(client: LocalityDaemonClient, domainId: String) {
        self.client = client
        self.mountId = nil
        self.containerIdentifier = nil
        self.domainId = domainId
        self.namespaceMountId = nil
        super.init()
    }

    init(empty: ()) {
        self.client = nil
        self.mountId = nil
        self.containerIdentifier = nil
        self.domainId = nil
        self.namespaceMountId = nil
        super.init()
    }

    func invalidate() {}

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        do {
            observer.didEnumerate(try currentItems())
            observer.finishEnumerating(upTo: nil)
        } catch {
            observer.finishEnumeratingWithError(agentFSFileProviderError(error))
        }
    }

    func currentSyncAnchor(
        completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void
    ) {
        completionHandler(currentSyncAnchor())
    }

    func enumerateChanges(
        for observer: NSFileProviderChangeObserver,
        from syncAnchor: NSFileProviderSyncAnchor
    ) {
        do {
            observer.didUpdate(try currentItems())
            observer.finishEnumeratingChanges(upTo: currentSyncAnchor(), moreComing: false)
        } catch {
            observer.finishEnumeratingWithError(agentFSFileProviderError(error))
        }
    }

    private func currentItems() throws -> [LocalityFileProviderItem] {
        guard let client else {
            return []
        }

        if let domainId {
            let response = try client.domainChildren(domainId: domainId)
            return response.children.map { child in
                LocalityFileProviderItem(metadata: child.item.namespaced(for: child.mountId))
            }
        }
        if let mountId, let containerIdentifier {
            let response = try client.children(
                mountId: mountId,
                containerIdentifier: containerIdentifier
            )
            return response.children.map { child in
                let metadata = namespaceMountId.map { child.namespaced(for: $0) } ?? child
                return LocalityFileProviderItem(metadata: metadata)
            }
        }
        return []
    }

    private func currentSyncAnchor() -> NSFileProviderSyncAnchor {
        NSFileProviderSyncAnchor(Data(Date().timeIntervalSince1970.description.utf8))
    }
}

func agentFSFileProviderError(_ error: Error) -> NSError {
    let nsError = error as NSError
    if nsError.domain == NSCocoaErrorDomain || nsError.domain == NSFileProviderErrorDomain {
        return nsError
    }
    return NSError(
        domain: NSFileProviderErrorDomain,
        code: NSFileProviderError.serverUnreachable.rawValue,
        userInfo: [NSLocalizedDescriptionKey: nsError.localizedDescription]
    )
}
