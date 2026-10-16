// SPDX-License-Identifier: GPL-3.0-only
#include "snow_shot/presentation/screenshotimagefileservice.h"
#include "snow_shot/presentation/screenshotmanualsave.h"
#include "snow_shot/presentation/screenrecordingfolder.h"
#include "snow_shot/storage/applicationstorage.h"
#include "snow_shot/storage/timevariables.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QTemporaryDir>
#include <QTimeZone>

#include <cstdlib>
#include <iostream>

namespace storage = snow_shot::storage;
namespace recording = snow_shot::presentation::recording;

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(EXIT_FAILURE);
    }
}

void timeDirectories(const QTemporaryDir& temporary) {
    const QDateTime before(QDate(2026, 12, 31), QTime(23, 59, 59), QTimeZone::UTC);
    const QDateTime after = before.addSecs(1);
    const QString pattern = temporary.filePath(QStringLiteral("output/{YYYY}/{MM}/{DD}"));
    require(storage::expandTimeVariables(QStringLiteral(" {YYYY-MM-DD_HH-mm-ss} "), before) ==
                    QStringLiteral("2026-12-31_23-59-59") &&
                storage::expandTimeVariables(QStringLiteral("{yyyy}/{YY}/{dd}"), after) ==
                    QStringLiteral("2027/27/01"),
            "shared expansion must preserve existing date formats and aliases");
    require(
        storage::ScreenshotSettings().setImageSaveDirectory(pattern) &&
            storage::ScreenshotSettings().setManualSaveFilenameFormat(QStringLiteral("{YYYY}")) &&
            storage::RecordingSettings().setVideoSaveDirectory(pattern),
        "template settings must persist");
    QImage image(QSize(2, 2), QImage::Format_RGB32);
    image.fill(Qt::red);
    for (const auto& timestamp : {before, after}) {
        const QString expanded = storage::expandTimeVariables(pattern, timestamp);
        require(!QFileInfo::exists(expanded), "dated directory must initially be absent");
        const auto imageDirectories =
            ScreenshotImageFileService::automaticDirectories(pattern, timestamp);
        const auto videoDirectories = recording::screenRecordingDirectories(timestamp);
        require(imageDirectories.constFirst() == expanded &&
                    videoDirectories.constFirst() == expanded && !QFileInfo::exists(expanded),
                "directory resolution must expand templates without filesystem mutation");
        const auto saved = ScreenshotImageFileService::saveAutomatically(
            image, imageDirectories, ScreenshotImageFileFormat::Png, QStringLiteral("{YYYY}"),
            timestamp);
        const auto defaults = screenshotManualSaveDefaults(timestamp);
        require(
            saved.succeeded() && saved.path == defaults.initialPath &&
                saved.path == QDir(expanded).filePath(timestamp.toString(QStringLiteral("yyyy")) +
                                                      QStringLiteral(".png")) &&
                !saved.path.contains(QLatin1Char('{')),
            "automatic and manual output must use one timestamp and create the dated directory");
        const QString baseName =
            ScreenshotImageFileService::suggestedBaseName(QStringLiteral("{YYYY}"), timestamp);
        const QString video =
            recording::screenRecordingOutputPath(videoDirectories, baseName, QStringLiteral("mp4"));
        require(video == QDir(expanded).filePath(baseName + QStringLiteral(".mp4")) &&
                    recording::screenRecordingDirectory(timestamp) == expanded,
                "recording output and folder resolution must use the expanded directory");
        QFile collision(video);
        require(collision.open(QIODevice::WriteOnly), "video collision fixture must open");
        collision.close();
        require(recording::screenRecordingOutputPath(videoDirectories, baseName,
                                                     QStringLiteral("mp4")) ==
                    QDir(expanded).filePath(baseName + QStringLiteral("_1.mp4")),
                "recording collision names must remain intact");
    }
    require(storage::ScreenshotSettings().imageSaveDirectory() == pattern &&
                storage::RecordingSettings().videoSaveDirectory() == pattern,
            "resolution must preserve stored templates");

    const QString videoPattern = temporary.filePath(QStringLiteral("videos/{YYYY}"));
    const QString videoDirectory = temporary.filePath(QStringLiteral("videos/2027"));
    require(storage::RecordingSettings().setVideoSaveDirectory(videoPattern) &&
                !QFileInfo::exists(videoDirectory),
            "new recording directory fixture must be absent");
    require(recording::screenRecordingOutputPath(recording::screenRecordingDirectories(after),
                                                 QStringLiteral("video"), QStringLiteral("mp4")) ==
                    QDir(videoDirectory).filePath(QStringLiteral("video.mp4")) &&
                QFileInfo(videoDirectory).isDir(),
            "first recording must create its dated output directory");

    const QString remembered = temporary.filePath(QStringLiteral("remembered/{YYYY}"));
    require(QDir().mkpath(remembered) && ScreenshotImageFileService::saveDialogDirectory(
                                             remembered, pattern, before) == remembered,
            "remembered concrete paths must remain literal and retain precedence");
    const QString missing = temporary.filePath(QStringLiteral("missing"));
    require(!ScreenshotImageFileService::automaticDirectories(missing, before).contains(missing) &&
                storage::RecordingSettings().setVideoSaveDirectory(missing) &&
                !recording::screenRecordingDirectories(before).contains(missing),
            "missing literal configured directories must retain their fallback behavior");
    const QString blocker = temporary.filePath(QStringLiteral("blocked"));
    QFile file(blocker);
    require(file.open(QIODevice::WriteOnly), "blocked directory fixture must open");
    file.close();
    const QString expandedBlocked =
        storage::expandTimeVariables(blocker + QStringLiteral("/{YYYY}"), before);
    const QString fallback = temporary.filePath(QStringLiteral("fallback"));
    const auto saved = ScreenshotImageFileService::saveAutomatically(
        image, {expandedBlocked, fallback}, ScreenshotImageFileFormat::Png,
        QStringLiteral("fallback"), before);
    require(saved.succeeded() && QFileInfo(saved.path).absolutePath() == fallback &&
                recording::screenRecordingOutputPath({expandedBlocked, fallback},
                                                     QStringLiteral("fallback"),
                                                     QStringLiteral("mp4")) ==
                    QDir(fallback).filePath(QStringLiteral("fallback.mp4")),
            "unusable expanded targets must retain writer fallback behavior");
}
} // namespace

int main(int argc, char** argv) {
    QGuiApplication application(argc, argv);
    QTemporaryDir temporary;
    auto& appStorage = storage::ApplicationStorage::instance();
    require(temporary.isValid() &&
                appStorage.initialize({temporary.path(), temporary.path(), 60000}).success,
            "isolated settings must initialize");
    timeDirectories(temporary);
    appStorage.shutdown();
    return EXIT_SUCCESS;
}
