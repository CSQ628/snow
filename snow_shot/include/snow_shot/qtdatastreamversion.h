#ifndef SNOW_SHOT_QTDATASTREAMVERSION_H
#define SNOW_SHOT_QTDATASTREAMVERSION_H

#include <QDataStream>
#include <QtGlobal>

namespace snow_shot {

// Persisted Snow Shot records pin the QDataStream version so records written
// by one Qt release keep decoding on another. The pinned value is
// QDataStream::Qt_6_11, which is 24.
//
// Qt 6.10 and older do not declare the Qt_6_11 enumerator, and the distro Qt
// on Linux is behind the audited 6.12 kit. The enum is an unscoped enum with
// an implicit int underlying type, and setVersion() takes an int, so the
// numeric value works on every supported Qt and produces identical bytes.
#if QT_VERSION >= QT_VERSION_CHECK(6, 11, 0)
inline constexpr int kQDataStreamVersion = QDataStream::Qt_6_11;
#else
inline constexpr int kQDataStreamVersion = 24;
#endif

} // namespace snow_shot

#endif // SNOW_SHOT_QTDATASTREAMVERSION_H
