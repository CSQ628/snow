#include "snow_shot/presentation/globalmousemanager.h"
#include "snow_shot/storage/applicationstorage.h"

#include <QJsonObject>
#include <QMutex>
#include <QMutexLocker>
#include <QThread>
#include <array>
#include <utility>

namespace snow_shot::presentation {
struct GlobalMouseManager::Impl {
    Impl(GlobalMouseManager& owner, std::unique_ptr<GlobalMouseBackend> input)
        : q(owner), backend(input ? std::move(input) : createGlobalMouseBackend()) {}

    void reload() {
        configuration.bindings.clear();
        using Action = settings::SettingsGlobalMouseAction;
        const std::array pairs{
            std::pair{Action::ScreenshotCopy, "global_mouse/screenshot_copy"},
            std::pair{Action::ScreenshotFixed, "global_mouse/screenshot_fixed"},
            std::pair{Action::ScreenshotOcr, "global_mouse/screenshot_ocr"},
            std::pair{Action::ScreenshotTranslation, "global_mouse/screenshot_translation"},
            std::pair{Action::ScreenshotSave, "global_mouse/screenshot_save"},
            std::pair{Action::ScreenshotQuickSave, "global_mouse/screenshot_quick_save"},
            std::pair{Action::ScreenRecording, "global_mouse/screen_recording"}};
        auto& store = storage::ApplicationStorage::instance().configuration();
        for (const auto& [action, key] : pairs) {
            const auto value = store.value(QString::fromLatin1(key)).toObject();
            const auto binding = globalMouseBinding(
                action, {value.value(QStringLiteral("activation_key")).toVariant().toStringList(),
                         value.value(QStringLiteral("mouse_button")).toString()});
            if (binding) {
                configuration.bindings.push_back(*binding);
            }
        }
        backend->configure(configuration);
    }

    void post(GlobalMouseDragEvent event, quint64 epoch) {
        QMutexLocker lock(&mutex);
        if (epoch != generation) {
            return;
        }
        using Kind = GlobalMouseDragEvent::Kind;
        if (event.kind == Kind::Update) {
            if (event.id == 0 || event.id != producerActiveId) {
                return;
            }
            const bool alreadyPending = pendingUpdate.has_value();
            pendingUpdate = event;
            if (alreadyPending) {
                return;
            }
            // Hold the notification until the presentation owner consumes its sample.
            updateNotificationDelivered = false;
        } else {
            if (event.kind == Kind::Begin) {
                producerActiveId = event.id;
                pendingUpdate.reset();
                updateNotificationDelivered = false;
            } else {
                if (producerActiveId == event.id) {
                    producerActiveId = 0;
                }
                clearPendingUpdate(event.id);
            }
            pending.push_back(event);
        }
        if (deliveryQueued) {
            return;
        }
        deliveryQueued = true;
        QMetaObject::invokeMethod(&q, [this, epoch]() { deliver(epoch); }, Qt::QueuedConnection);
    }

    void clearPendingUpdate(quint64 id) {
        if (pendingUpdate && pendingUpdate->id == id) {
            pendingUpdate.reset();
            updateNotificationDelivered = false;
        }
    }

    void deliver(quint64 epoch) {
        QVector<GlobalMouseDragEvent> events;
        {
            QMutexLocker lock(&mutex);
            if (epoch != generation || !started) {
                return;
            }
            deliveryQueued = false;
            events.swap(pending);
        }
        for (const auto& item : events) {
            if (epoch != generation || !started) {
                return;
            }
            if (item.kind == GlobalMouseDragEvent::Kind::Begin) {
                activeId = item.id;
                pendingCaptureId = item.id;
                pendingCaptureEvent = item;
            } else if (item.kind == GlobalMouseDragEvent::Kind::Cancel &&
                       pendingCaptureId == item.id) {
                activeId = 0;
                pendingCaptureId = 0;
            } else if (activeId != item.id) {
                continue;
            } else {
                activeId = 0;
            }
            emit q.dragEvent(item);
        }
        if (epoch != generation || !started) {
            return;
        }
        quint64 updateId = 0;
        {
            QMutexLocker lock(&mutex);
            if (pendingUpdate && pendingUpdate->id == activeId && !updateNotificationDelivered) {
                updateNotificationDelivered = true;
                updateId = pendingUpdate->id;
            }
        }
        if (updateId != 0) {
            emit q.dragUpdateAvailable(updateId);
        }
    }

    GlobalMouseManager& q;
    std::unique_ptr<GlobalMouseBackend> backend;
    GlobalMouseConfiguration configuration;
    GlobalMousePermissionState permission;
    QMetaObject::Connection configurationConnection;
    QMutex mutex;
    QVector<GlobalMouseDragEvent> pending;
    std::optional<GlobalMouseDragEvent> pendingUpdate;
    quint64 generation = 0;
    quint64 producerActiveId = 0;
    quint64 activeId = 0;
    // Release ends movement, but capture preparation may still need cancellation.
    quint64 pendingCaptureId = 0;
    GlobalMouseDragEvent pendingCaptureEvent;
    bool updateNotificationDelivered = false;
    bool deliveryQueued = false;
    bool started = false;
};

GlobalMouseManager::GlobalMouseManager(QObject* parent) : GlobalMouseManager(nullptr, parent) {}
GlobalMouseManager::GlobalMouseManager(std::unique_ptr<GlobalMouseBackend> backend, QObject* parent)
    : QObject(parent), m_impl(std::make_unique<Impl>(*this, std::move(backend))) {}
GlobalMouseManager::~GlobalMouseManager() {
    disconnect(this, nullptr, nullptr, nullptr);
    shutdown();
}

void GlobalMouseManager::initialize() {
    auto& impl = *m_impl;
    if (impl.started) {
        return;
    }
    impl.started = true;
    const quint64 epoch = impl.generation;
    impl.reload();
    impl.configurationConnection =
        connect(&storage::ApplicationStorage::instance().configuration(),
                &storage::ConfigurationStore::valueChanged, this,
                [this](const QString& key, const QJsonValue&) {
                    if (key.startsWith(QStringLiteral("global_mouse/"))) {
                        m_impl->reload();
                    }
                });
    impl.backend->setPermissionRefreshHandler([this, epoch] {
        QMetaObject::invokeMethod(
            this,
            [this, epoch] {
                if (m_impl->started && epoch == m_impl->generation)
                    emit permissionRefreshRequested();
            },
            Qt::QueuedConnection);
    });
    impl.backend->setStateHandler([this, epoch](GlobalMousePermissionState state) {
        QMetaObject::invokeMethod(
            this,
            [this, epoch, state] {
                if (!m_impl->started || epoch != m_impl->generation || m_impl->permission == state)
                    return;
                m_impl->permission = state;
                emit permissionStateChanged(state);
                using Status = GlobalMousePermissionState::Status;
                if (state.status == Status::ListenRequired ||
                    state.status == Status::AccessibilityRequired ||
                    state.status == Status::Unavailable)
                    emit operationFailed(globalMousePermissionMessage(state));
            },
            Qt::QueuedConnection);
    });
    impl.permission = impl.backend->permissionState();
    emit permissionStateChanged(impl.permission);
    impl.backend->start(
        [this, epoch](GlobalMouseDragEvent event) { m_impl->post(event, epoch); },
        [this, epoch](quint32 code) {
            QMetaObject::invokeMethod(
                this,
                [this, code, epoch]() {
                    if (m_impl->started && epoch == m_impl->generation) {
                        qWarning("Global mouse input initialization failed: %u", code);
                        emit operationFailed(
                            tr("Global mouse input is unavailable (error %1).").arg(code));
                    }
                },
                Qt::QueuedConnection);
        });
}

void GlobalMouseManager::beginButtonDrag(settings::SettingsGlobalMouseAction action) {
    if (m_impl->started && m_impl->configuration.captureAvailable) {
        m_impl->backend->beginButtonDrag(action);
    }
}

std::optional<GlobalMouseDragEvent> GlobalMouseManager::takePendingUpdate(quint64 id) {
    Q_ASSERT(QThread::currentThread() == thread());
    QMutexLocker lock(&m_impl->mutex);
    if (!m_impl->started || m_impl->activeId != id || !m_impl->pendingUpdate ||
        m_impl->pendingUpdate->id != id) {
        return std::nullopt;
    }
    auto event = std::exchange(m_impl->pendingUpdate, std::nullopt);
    m_impl->updateNotificationDelivered = false;
    return event;
}

GlobalMousePermissionState GlobalMouseManager::permissionState() const {
    return m_impl->permission;
}

void GlobalMouseManager::usePermissionSnapshot(bool listen, bool accessibility) {
    m_impl->backend->usePermissionSnapshot(listen, accessibility);
}
void GlobalMouseManager::refreshPermission() {
    m_impl->backend->refreshPermission();
}

void GlobalMouseManager::requestPermission() {
    m_impl->backend->requestPermission();
    refreshPermission();
}

void GlobalMouseManager::openPermissionSettings() {
    m_impl->backend->openPermissionSettings();
}

void GlobalMouseManager::shutdown() {
    if (!m_impl->started) {
        return;
    }
    const auto pendingId = m_impl->pendingCaptureId;
    auto cancellation = m_impl->pendingCaptureEvent;
    cancellation.kind = GlobalMouseDragEvent::Kind::Cancel;
    m_impl->started = false;
    disconnect(m_impl->configurationConnection);
    m_impl->backend->stop();
    {
        QMutexLocker lock(&m_impl->mutex);
        ++m_impl->generation;
        m_impl->activeId = 0;
        m_impl->producerActiveId = 0;
        m_impl->pendingCaptureId = 0;
        m_impl->pendingUpdate.reset();
        m_impl->updateNotificationDelivered = false;
        m_impl->deliveryQueued = false;
        m_impl->pending.clear();
    }
    m_impl->permission = {};
    emit permissionStateChanged(m_impl->permission);
    // Complete teardown before invoking subscribers, which may restart the manager.
    if (pendingId != 0)
        emit dragEvent(cancellation);
}

void GlobalMouseManager::setCaptureAvailable(bool available) {
    m_impl->configuration.captureAvailable = available;
    m_impl->backend->configure(m_impl->configuration);
}
void GlobalMouseManager::cancelGesture(quint64 id) {
    if (m_impl->pendingCaptureId == id) {
        m_impl->pendingCaptureId = 0;
        m_impl->activeId = 0;
    }
    {
        QMutexLocker lock(&m_impl->mutex);
        if (m_impl->producerActiveId == id) {
            m_impl->producerActiveId = 0;
        }
        m_impl->clearPendingUpdate(id);
        m_impl->pending.removeIf([id](const auto& event) { return event.id == id; });
    }
    m_impl->backend->cancel(id);
}
QString globalMousePermissionMessage(const GlobalMousePermissionState& state) {
    using Status = GlobalMousePermissionState::Status;
    switch (state.status) {
    case Status::Ready:
        return GlobalMouseManager::tr("Global mouse gestures are ready.");
    case Status::ListenRequired:
        return GlobalMouseManager::tr("Allow Snow Shot in System Settings > Privacy & Security > "
                                      "Input Monitoring to use global mouse gestures.");
    case Status::AccessibilityRequired:
        return GlobalMouseManager::tr("Allow Snow Shot in System Settings > Privacy & Security > "
                                      "Accessibility to use global mouse gestures.");
    case Status::Unavailable:
        return GlobalMouseManager::tr(
            "Global mouse input is unavailable. Check permissions and retry.");
    case Status::Suspended:
        return GlobalMouseManager::tr(
            "Global mouse gestures are paused while this session is inactive.");
    case Status::Unknown:
        return GlobalMouseManager::tr("Checking global mouse permissions...");
    }
    return {};
}
} // namespace snow_shot::presentation
