pub const CANVAS_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ZeroClaw Live Canvas 🦀</title>
    <style>
        :root {
            --bg: #0a0a0b;
            --surface: #161618;
            --primary: #5c6bc0;
            --text: #e0e0e0;
            --text-dim: #9e9e9e;
            --border: #2d2d30;
        }

        body, html {
            margin: 0;
            padding: 0;
            height: 100%;
            background: var(--bg);
            color: var(--text);
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            overflow: hidden;
        }

        #canvas-container {
            width: 100%;
            height: 100%;
            display: flex;
            flex-direction: column;
            position: relative;
        }

        #canvas-header {
            height: 60px;
            background: var(--surface);
            border-bottom: 1px solid var(--border);
            display: flex;
            align-items: center;
            padding: 0 24px;
            justify-content: space-between;
            z-index: 100;
        }

        .logo {
            font-size: 1.2rem;
            font-weight: 700;
            color: #fff;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .status {
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 0.85rem;
            color: var(--text-dim);
        }

        .status-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background: #4caf50;
            box-shadow: 0 0 8px #4caf5055;
        }

        .status-dot.offline {
            background: #f44336;
            box-shadow: 0 0 8px #f4433655;
        }

        #canvas-content {
            flex: 1;
            padding: 24px;
            overflow-y: auto;
            position: relative;
            background-image: radial-gradient(var(--border) 1px, transparent 1px);
            background-size: 30px 30px;
        }

        /* Responsive styling */
        @media (max-width: 768px) {
            #canvas-content { padding: 12px; }
        }

        /* Micro-animations for updates */
        .canvas-updated {
            animation: fadeIn 0.4s ease-out;
        }

        @keyframes fadeIn {
            from { opacity: 0; transform: translateY(10px); }
            to { opacity: 1; transform: translateY(0); }
        }

        #custom-css { display: none; }
    </style>
    <style id="dynamic-css"></style>
</head>
<body>
    <div id="canvas-container">
        <header id="canvas-header">
            <div class="logo">
                <span>ZeroClaw</span> <span style="opacity: 0.6; font-weight: 300;">Live Canvas</span>
            </div>
            <div class="status">
                <div id="status-dot" class="status-dot offline"></div>
                <span id="status-text">Disconnected</span>
            </div>
        </header>
        <main id="canvas-content">
            <div id="renderer">
                <!-- Content will be injected here -->
            </div>
        </main>
    </div>

    <script>
        const renderer = document.getElementById('renderer');
        const dynamicCss = document.getElementById('dynamic-css');
        const statusDot = document.getElementById('status-dot');
        const statusText = document.getElementById('status-text');

        let ws;
        let reconnectAttempts = 0;

        function connect() {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${protocol}//${window.location.host}/canvas/ws`;
            
            ws = new WebSocket(wsUrl);

            ws.onopen = () => {
                console.log('Connected to ZeroClaw Canvas Gateway');
                statusDot.classList.remove('offline');
                statusText.textContent = 'Live';
                reconnectAttempts = 0;
            };

            ws.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    updateCanvas(data);
                } catch (e) {
                    console.error('Failed to parse canvas update:', e);
                }
            };

            ws.onclose = () => {
                statusDot.classList.add('offline');
                statusText.textContent = 'Disconnected';
                
                // Exponential backoff reconnect
                const delay = Math.min(1000 * Math.pow(2, reconnectAttempts), 10000);
                reconnectAttempts++;
                console.log(`Connection lost. Reconnecting in ${delay}ms...`);
                setTimeout(connect, delay);
            };

            ws.onerror = (err) => {
                console.error('WebSocket error:', err);
                ws.close();
            };
        }

        function updateCanvas(data) {
            // Apply CSS if provided
            if (data.css !== undefined && data.css !== null) {
                dynamicCss.textContent = data.css;
            }

            // Update HTML with a fade-in effect if it changed significantly
            if (renderer.innerHTML !== data.html) {
                renderer.classList.remove('canvas-updated');
                void renderer.offsetWidth; // trigger reflow
                renderer.innerHTML = data.html;
                renderer.classList.add('canvas-updated');
            }
        }

        // Initial connection
        connect();

        // Check for initial state
        fetch('/canvas/state')
            .then(res => res.json())
            .then(data => updateCanvas(data))
            .catch(err => console.error('Failed to fetch initial state:', err));
    </script>
</body>
</html>
"#;
