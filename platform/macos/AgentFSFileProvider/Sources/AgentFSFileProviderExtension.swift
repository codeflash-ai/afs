@preconcurrency import FileProvider
@preconcurrency import Foundation

final class AgentFSFileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let domain: NSFileProviderDomain
    private let client: Result<AgentFSDaemonClient, Error>

    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        self.client = Result {
            try AgentFSDaemonClient()
        }
        super.init()
    }

    func invalidate() {}

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let client = try self.daemonClient()
                let daemonIdentifier = AgentFSFileProviderItem.daemonIdentifier(identifier)
                let response = try client.item(
                    mountId: self.mountId,
                    identifier: daemonIdentifier
                )
                completionHandler(AgentFSFileProviderItem(metadata: response.item), nil)
                progress.completedUnitCount = 1
            } catch {
                completionHandler(nil, error)
            }
        }
        return progress
    }

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> NSFileProviderEnumerator {
        let client = try daemonClient()
        let daemonIdentifier = AgentFSFileProviderItem.daemonIdentifier(containerItemIdentifier)
        return AgentFSEnumerator(
            client: client,
            mountId: mountId,
            containerIdentifier: daemonIdentifier
        )
    }

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let client = try self.daemonClient()
                let daemonIdentifier = AgentFSFileProviderItem.daemonIdentifier(itemIdentifier)
                let materialized = try client.materialize(
                    mountId: self.mountId,
                    identifier: daemonIdentifier
                )
                let item = try client.item(
                    mountId: self.mountId,
                    identifier: daemonIdentifier
                )
                completionHandler(
                    URL(fileURLWithPath: materialized.path),
                    AgentFSFileProviderItem(metadata: item.item),
                    nil
                )
                progress.completedUnitCount = 1
            } catch {
                completionHandler(nil, nil, error)
            }
        }
        return progress
    }

    func createItem(
        basedOn itemTemplate: NSFileProviderItem,
        fields: NSFileProviderItemFields,
        contents url: URL?,
        options: NSFileProviderCreateItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        completionHandler(nil, [], false, unsupportedWriteError())
        progress.completedUnitCount = 1
        return progress
    }

    func modifyItem(
        _ item: NSFileProviderItem,
        baseVersion version: NSFileProviderItemVersion,
        changedFields: NSFileProviderItemFields,
        contents newContents: URL?,
        options: NSFileProviderModifyItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        completionHandler(nil, [], false, unsupportedWriteError())
        progress.completedUnitCount = 1
        return progress
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        completionHandler(unsupportedWriteError())
        progress.completedUnitCount = 1
        return progress
    }

    private var mountId: String {
        domain.identifier.rawValue
    }

    private func daemonClient() throws -> AgentFSDaemonClient {
        try client.get()
    }

    private func unsupportedWriteError() -> NSError {
        NSError(
            domain: NSCocoaErrorDomain,
            code: NSFeatureUnsupportedError,
            userInfo: [
                NSLocalizedDescriptionKey: "AgentFS File Provider writes are routed through the daemon push pipeline in a later slice.",
            ]
        )
    }
}
