/*
 * Unix signal watcher for Qt.
 *
 * Copyright (C) 2014 Simon Knopp
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

#ifndef SIGWATCH_H
#define SIGWATCH_H

#include <QObject>
#include <signal.h>

class UnixSignalWatcherPrivate;


/*!
 * \brief The UnixSignalWatcher class converts Unix signals to Qt signals.
 *
 * To watch for a given signal, e.g. \c SIGINT, call \c watchForSignal(SIGINT)
 * and \c connect() your handler to unixSignal().
 */

class UnixSignalWatcher : public QObject
{
    Q_OBJECT
public:
    explicit UnixSignalWatcher(QObject *parent = 0);
    ~UnixSignalWatcher();

    void watchForSignal(int signal);

signals:
    void unixSignal(int signal);

private:
    UnixSignalWatcherPrivate * const d_ptr;
    Q_DECLARE_PRIVATE(UnixSignalWatcher)
    Q_PRIVATE_SLOT(d_func(), void _q_onNotify(int))
};

#endif // SIGWATCH_H
