import GLib from 'gi://GLib';
import AstalIO from 'gi://AstalIO';

// Dispatch, response serialization and request ordering belong to each panel.
export function replyAndClose(connection, response, {
    label = 'Panel', asyncClose = true,
    onError = (phase, error) => console.warn(`${label} request ${phase}: ${error.message || String(error)}`),
} = {}) {
    const report = (phase, error) => {
        try { onError(phase, error); } catch (_) { /* Logging must not interrupt connection cleanup. */ }
    };
    let closing = false;
    const close = () => {
        if (closing) return;
        closing = true;
        try {
            if (asyncClose) connection.close_async(GLib.PRIORITY_DEFAULT, null, (_source, result) => {
                try { connection.close_finish(result); }
                catch (error) { report('close', error); }
            });
            else connection.close(null);
        } catch (error) { report('close', error); }
    };
    try {
        AstalIO.write_sock(connection, response, (_source, result) => {
            try { AstalIO.write_sock_finish(result); }
            catch (error) { report('reply', error); }
            finally { close(); }
        });
    } catch (error) {
        report('reply', error);
        close();
    }
}
