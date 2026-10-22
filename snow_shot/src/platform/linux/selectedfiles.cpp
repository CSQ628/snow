#include "snow_shot/platform/selectedfiles.h"

namespace snow_shot::platform {
namespace {
class LinuxSelectedFileBackend final : public SelectedFileBackend {
  public:
    SelectedFileTarget captureTarget() const override {
        return {};
    }

    SelectedFileResult selectedFiles(const SelectedFileTarget& target,
                                     const std::function<bool()>& cancelled) const override {
        Q_UNUSED(target);
        Q_UNUSED(cancelled);
        return {};
    }
};
} // namespace

std::shared_ptr<SelectedFileBackend> createSelectedFileBackend() {
    return std::make_shared<LinuxSelectedFileBackend>();
}
} // namespace snow_shot::platform
