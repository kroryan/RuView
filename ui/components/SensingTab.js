/**
 * SensingTab — Live WiFi Sensing Visualization
 *
 * Connects to the sensing WebSocket service and renders:
 *   1. A 3D Gaussian-splat signal field (via gaussian-splats.js)
 *   2. An overlay HUD with real-time metrics (RSSI, variance, bands, classification)
 */

import { sensingService } from '../services/sensing.service.js';
import { apiService } from '../services/api.service.js';
import { GaussianSplatRenderer } from './gaussian-splats.js';

export class SensingTab {
  /** @param {HTMLElement} container - the #sensing section element */
  constructor(container) {
    this.container = container;
    this.splatRenderer = null;
    this._unsubData = null;
    this._unsubState = null;
    this._resizeObserver = null;
    this._threeLoaded = false;
    this._calibrationTimer = null;
    this._calibrationIdentity = null;
  }

  async init() {
    this._buildDOM();
    await this._loadThree();
    this._initSplatRenderer();
    this._connectService();
    this._setupCalibration();
    this._refreshCalibrationStatus();
    this._setupResize();
  }

  // ---- DOM construction --------------------------------------------------

  _buildDOM() {
    this.container.innerHTML = `
      <h2>Live WiFi Sensing</h2>

      <!-- Data-source status banner — updated by _onStateChange -->
      <div id="sensingSourceBanner" class="sensing-source-banner sensing-source-reconnecting"
           role="status" aria-live="polite">
        RECONNECTING...
      </div>

      <div class="sensing-layout">
        <!-- 3D viewport -->
        <div class="sensing-viewport" id="sensingViewport">
          <div class="sensing-loading">Loading 3D engine...</div>
        </div>

        <!-- Side panel -->
        <div class="sensing-panel">
          <!-- Connection -->
          <div class="sensing-card">
            <div class="sensing-card-title">Connection</div>
            <div class="sensing-connection">
              <span class="sensing-dot" id="sensingDot"></span>
              <span id="sensingState">Connecting...</span>
              <span class="sensing-source" id="sensingSource"></span>
            </div>
          </div>

          <!-- RSSI -->
          <div class="sensing-card">
            <div class="sensing-card-title">RSSI</div>
            <div class="sensing-big-value" id="sensingRssi">-- dBm</div>
            <canvas id="sensingSparkline" width="200" height="40"></canvas>
          </div>

          <!-- Signal Features -->
          <div class="sensing-card">
            <div class="sensing-card-title">Signal Features</div>
            <div class="sensing-meters">
              <div class="sensing-meter">
                <label>Variance</label>
                <div class="sensing-bar"><div class="sensing-bar-fill" id="barVariance"></div></div>
                <span class="sensing-meter-val" id="valVariance">0</span>
              </div>
              <div class="sensing-meter">
                <label>Motion Band</label>
                <div class="sensing-bar"><div class="sensing-bar-fill motion" id="barMotion"></div></div>
                <span class="sensing-meter-val" id="valMotion">0</span>
              </div>
              <div class="sensing-meter">
                <label>Breathing Band</label>
                <div class="sensing-bar"><div class="sensing-bar-fill breath" id="barBreath"></div></div>
                <span class="sensing-meter-val" id="valBreath">0</span>
              </div>
              <div class="sensing-meter">
                <label>Spectral Power</label>
                <div class="sensing-bar"><div class="sensing-bar-fill spectral" id="barSpectral"></div></div>
                <span class="sensing-meter-val" id="valSpectral">0</span>
              </div>
            </div>
          </div>

          <!-- Classification -->
          <div class="sensing-card">
            <div class="sensing-card-title">Classification</div>
            <div class="sensing-classification" id="sensingClassification">
              <div class="sensing-class-label" id="classLabel">ABSENT</div>
              <div class="sensing-confidence">
                <label>Confidence</label>
                <div class="sensing-bar"><div class="sensing-bar-fill confidence" id="barConfidence"></div></div>
                <span class="sensing-meter-val" id="valConfidence">0%</span>
              </div>
            </div>
          </div>

          <!-- Setup info -->
          <div class="sensing-card">
            <div class="sensing-card-title">About This Data</div>
            <p class="sensing-about-text">
              Metrics are computed from WiFi Channel State Information (CSI).
              With <strong><span id="sensingNodeCount">0</span> ESP32 node(s)</strong> you get presence detection, breathing
              estimation, and gross motion. Add <strong>3-4+ ESP32 nodes</strong>
              around the room for spatial resolution and limb-level tracking.
            </p>
          </div>

          <!-- Node Status -->
          <div class="sensing-card" id="sensingNodeCards">
            <div class="sensing-card-title">NODE STATUS</div>
            <div id="nodeStatusContainer"></div>
          </div>

          <!-- Real empty-room field calibration -->
          <div class="sensing-card sensing-calibration-card" id="sensingCalibrationCard">
            <div class="sensing-card-title">ROOM CALIBRATION — LIVE PROGRESS</div>
            <p class="sensing-about-text">
              Keep the monitored room empty. One capture session can bind
              several live ESP32 nodes together; every selected node must
              contribute real CSI before finalization. It never generates demo data.
            </p>
            <label for="calibrationNodeIds">Node IDs (comma separated — example: 1,2,3)</label>
            <input id="calibrationNodeIds" class="sensing-calibration-input" value="1,2,3" placeholder="Example: 1,2,3" inputmode="numeric" autocomplete="off">
            <div class="sensing-calibration-actions">
              <button id="calibrationUseLive" class="sensing-calibration-button">Use all live nodes</button>
              <button id="calibrationStart" class="sensing-calibration-button">Start empty-room capture</button>
              <button id="calibrationStop" class="sensing-calibration-button" disabled>Finalize</button>
              <button id="calibrationReset" class="sensing-calibration-button sensing-calibration-danger">Reset</button>
            </div>
            <div class="sensing-calibration-progress" aria-live="polite">
              <div class="sensing-calibration-progress-heading">
                <strong id="calibrationProgressPercent">0%</strong>
                <span>GLOBAL COMPLETION</span>
              </div>
              <div class="sensing-calibration-progress-track">
                <div id="calibrationProgress" class="sensing-calibration-progress-fill" style="width:0%"></div>
              </div>
              <div class="sensing-calibration-stats">
                <div><span>ELAPSED</span><strong id="calibrationElapsed">00:00</strong></div>
                <div><span>TARGET</span><strong id="calibrationTarget">--:--</strong></div>
                <div><span>NODES</span><strong id="calibrationNodeProgress">0 / 0</strong></div>
                <div><span>FRAMES</span><strong id="calibrationFrameProgress">0 / 0</strong></div>
              </div>
              <div id="calibrationProgressText" class="sensing-calibration-progress-text">No active calibration.</div>
            </div>
            <div id="calibrationStatus" class="sensing-calibration-status" role="status" aria-live="polite">
              Waiting for ESP32 frames.
            </div>
          </div>

          <!-- Extra info -->
          <div class="sensing-card">
            <div class="sensing-card-title">Details</div>
            <div class="sensing-details">
              <div class="sensing-detail-row">
                <span>Dominant Freq</span><span id="valDomFreq">0 Hz</span>
              </div>
              <div class="sensing-detail-row">
                <span>Change Points</span><span id="valChangePoints">0</span>
              </div>
              <div class="sensing-detail-row">
                <span>Sample Rate</span><span id="valSampleRate">--</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    `;
  }

  // ---- Three.js loading --------------------------------------------------

  async _loadThree() {
    if (window.THREE) {
      this._threeLoaded = true;
      return;
    }

    return new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = 'https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js';
      script.onload = () => {
        this._threeLoaded = true;
        resolve();
      };
      script.onerror = () => reject(new Error('Failed to load Three.js'));
      document.head.appendChild(script);
    });
  }

  // ---- Splat renderer ----------------------------------------------------

  _initSplatRenderer() {
    const viewport = this.container.querySelector('#sensingViewport');
    if (!viewport) return;

    // Remove loading message
    viewport.innerHTML = '';

    try {
      this.splatRenderer = new GaussianSplatRenderer(viewport, {
        width: viewport.clientWidth,
        height: viewport.clientHeight || 500,
      });
    } catch (e) {
      console.error('[SensingTab] Failed to init splat renderer:', e);
      viewport.innerHTML = '<div class="sensing-loading">3D rendering unavailable</div>';
    }
  }

  // ---- Service connection ------------------------------------------------

  _connectService() {
    sensingService.start();

    this._unsubData = sensingService.onData((data) => this._onSensingData(data));
    this._unsubState = sensingService.onStateChange((state) => this._onStateChange(state));
  }

  _onSensingData(data) {
    // Update 3D view
    if (this.splatRenderer) {
      this.splatRenderer.update(data);
    }

    // Update HUD
    this._updateHUD(data);

    // Update per-node panels
    this._updateNodePanels(data);
  }

  _onStateChange(state) {
    const dot    = this.container.querySelector('#sensingDot');
    const text   = this.container.querySelector('#sensingState');
    const banner = this.container.querySelector('#sensingSourceBanner');

    if (dot && text) {
      const stateLabels = {
        disconnected: 'Disconnected',
        connecting:   'Connecting...',
        connected:    'Connected',
        reconnecting: 'Reconnecting...',
        simulated:    'Simulated',
        'auth-required': 'API token required',
      };
      dot.className = 'sensing-dot ' + state;
      text.textContent = stateLabels[state] || state;
    }

    if (banner) {
      // Map the service's dataSource to banner text and CSS modifier class.
      const dataSource = sensingService.dataSource;
      const bannerConfig = {
        'live':              { text: 'LIVE \u2014 ESP32 HARDWARE',           cls: 'sensing-source-live' },
        'server-simulated':  { text: 'SIMULATED \u2014 NO HARDWARE',        cls: 'sensing-source-server-sim' },
        'waiting-for-hardware': { text: 'ESP32 CONFIGURED \u2014 WAITING FOR HARDWARE DATA', cls: 'sensing-source-waiting' },
        'reconnecting':      { text: 'RECONNECTING...',                    cls: 'sensing-source-reconnecting' },
        'unreachable':       { text: 'NO DATA \u2014 SERVER UNREACHABLE',   cls: 'sensing-source-simulated' },
        'simulated':         { text: 'INVENTED DATA \u2014 NOT MEASURED',   cls: 'sensing-source-simulated' },
        'auth-required':     { text: 'API TOKEN REQUIRED \u2014 SETTINGS \u2192 API ACCESS', cls: 'sensing-source-simulated' },
      };
      const cfg = bannerConfig[dataSource] || bannerConfig.reconnecting;
      banner.textContent = cfg.text;
      banner.className = 'sensing-source-banner ' + cfg.cls;
    }
  }

  _setupCalibration() {
    const useLive = this.container.querySelector('#calibrationUseLive');
    const start = this.container.querySelector('#calibrationStart');
    const stop = this.container.querySelector('#calibrationStop');
    const reset = this.container.querySelector('#calibrationReset');
    if (!start || !stop || !reset) return;
    useLive?.addEventListener('click', () => void this._useLiveCalibrationNodes());
    start.addEventListener('click', () => void this._startCalibration());
    stop.addEventListener('click', () => void this._stopCalibration());
    reset.addEventListener('click', () => void this._resetCalibration());
  }

  async _useLiveCalibrationNodes() {
    try {
      const latest = await apiService.get('/api/v1/sensing/latest');
      const ids = [...new Set((latest?.nodes || [])
        .map((node) => Number(node.node_id))
        .filter((id) => Number.isInteger(id) && id >= 0 && id <= 255))]
        .sort((a, b) => a - b);
      if (!ids.length) throw new Error('No live ESP32 nodes are available yet.');
      const input = this.container.querySelector('#calibrationNodeIds');
      if (input) input.value = ids.join(',');
      this._setCalibrationStatus(`Selected live nodes: ${ids.join(', ')}. Keep the room empty and start capture.`);
    } catch (error) {
      this._setCalibrationStatus(error.message, true);
    }
  }

  _calibrationNodeIds() {
    const input = this.container.querySelector('#calibrationNodeIds');
    const ids = String(input?.value || '')
      .split(',')
      .map(value => Number(value.trim()))
      .filter(Number.isInteger);
    const unique = [...new Set(ids)].sort((a, b) => a - b);
    if (!unique.length || unique.some(id => id < 0 || id > 255)) {
      throw new Error('Enter one or more node IDs from 0 to 255.');
    }
    return unique;
  }

  async _roomBindingDigest(nodeIds) {
    const material = `ruview-room-nodes-${nodeIds.join('-')}`;
    const bytes = new TextEncoder().encode(material);
    const digest = await crypto.subtle.digest('SHA-256', bytes);
    return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
  }

  _setCalibrationStatus(text, error = false) {
    const element = this.container.querySelector('#calibrationStatus');
    if (element) {
      element.textContent = text;
      element.classList.toggle('sensing-calibration-error', error);
    }
  }

  _setCalibrationButtons(active) {
    const start = this.container.querySelector('#calibrationStart');
    const stop = this.container.querySelector('#calibrationStop');
    if (start) start.disabled = active;
    if (stop) stop.disabled = !active;
  }

  _formatCalibrationTime(seconds) {
    const total = Math.max(0, Math.floor(Number(seconds) || 0));
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    const secs = total % 60;
    return `${hours ? `${String(hours).padStart(2, '0')}:` : ''}${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
  }

  _setCalibrationProgress(status) {
    const selected = (status?.source_node_ids || this._calibrationIdentity?.source_node_ids || [])
      .map(Number).filter(Number.isInteger);
    const observed = new Set((status?.observed_source_node_ids || []).map(Number));
    const nodeTotal = selected.length;
    const nodeDone = selected.filter((id) => observed.has(id)).length;
    const frameTarget = Number(status?.min_frames || 0);
    const durationTarget = Number(status?.min_duration_s || 0);
    const frameProgress = frameTarget > 0 ? Math.min(1, Number(status?.frame_count || 0) / frameTarget) : 0;
    const durationProgress = durationTarget > 0 ? Math.min(1, Number(status?.elapsed_s || 0) / durationTarget) : 0;
    const nodeProgress = nodeTotal > 0 ? nodeDone / nodeTotal : 0;
    // Global completion is gated by every selected node plus both server gates.
    const progress = nodeTotal > 0 ? Math.round(Math.min(frameProgress, durationProgress, nodeProgress) * 100) : 0;
    const bar = this.container.querySelector('#calibrationProgress');
    const percent = this.container.querySelector('#calibrationProgressPercent');
    const elapsedEl = this.container.querySelector('#calibrationElapsed');
    const targetEl = this.container.querySelector('#calibrationTarget');
    const nodeEl = this.container.querySelector('#calibrationNodeProgress');
    const frameEl = this.container.querySelector('#calibrationFrameProgress');
    const text = this.container.querySelector('#calibrationProgressText');
    if (bar) bar.style.width = `${progress}%`;
    if (percent) percent.textContent = `${progress}%`;
    if (elapsedEl) elapsedEl.textContent = this._formatCalibrationTime(status?.elapsed_s);
    if (targetEl) targetEl.textContent = durationTarget > 0 ? this._formatCalibrationTime(durationTarget) : '--:--';
    if (nodeEl) nodeEl.textContent = `${nodeDone} / ${nodeTotal}`;
    if (frameEl) frameEl.textContent = `${Number(status?.frame_count || 0).toLocaleString()} / ${frameTarget ? frameTarget.toLocaleString() : '--'}`;
    if (text) {
      const elapsed = this._formatCalibrationTime(status?.elapsed_s);
      const target = durationTarget ? ` / ${this._formatCalibrationTime(durationTarget)}` : '';
      const missing = selected.filter((id) => !observed.has(id));
      if (nodeTotal === 0) {
        text.textContent = 'No active calibration. Select the live node IDs and start an empty-room capture.';
      } else if (missing.length) {
        text.textContent = `Waiting for nodes: ${missing.join(', ')} — ${elapsed}${target}`;
      } else {
        text.textContent = `All selected nodes are contributing — ${elapsed}${target}`;
      }
    }
  }

  async _startCalibration() {
    try {
      const nodeIds = this._calibrationNodeIds();
      const digest = await this._roomBindingDigest(nodeIds);
      const query = nodeIds.length === 1 ? `?source_node_id=${nodeIds[0]}` : '';
      const result = await apiService.post(`/api/v1/calibration/start${query}`, {
        binding_digest: digest,
        source_node_ids: nodeIds,
      });
      if (!result?.success) throw new Error(result?.error || 'Calibration could not start.');
      this._calibrationIdentity = {
        boot_epoch: result.boot_epoch,
        session_id: result.session_id,
        binding_digest: result.binding_digest || digest,
        source_node_ids: nodeIds,
      };
      this._setCalibrationButtons(true);
      this._setCalibrationStatus('Capture started. Keep the room empty for at least 10 minutes.');
      this._startCalibrationPolling();
      await this._refreshCalibrationStatus();
    } catch (error) {
      this._setCalibrationStatus(error.message, true);
    }
  }

  async _stopCalibration() {
    try {
      if (!this._calibrationIdentity) await this._refreshCalibrationStatus();
      if (!this._calibrationIdentity) throw new Error('No active calibration identity is available.');
      const result = await apiService.post('/api/v1/calibration/stop', this._calibrationIdentity);
      if (!result?.success) throw new Error(result?.error || 'Calibration is not complete yet.');
      this._stopCalibrationPolling();
      this._setCalibrationProgress({
        source_node_ids: result.source_node_ids || this._calibrationIdentity.source_node_ids,
        observed_source_node_ids: result.source_node_ids || this._calibrationIdentity.source_node_ids,
        frame_count: result.frame_count,
        min_frames: result.frame_count,
        elapsed_s: result.elapsed_s,
        min_duration_s: result.elapsed_s,
      });
      this._calibrationIdentity = null;
      this._setCalibrationButtons(false);
      this._setCalibrationStatus(`Calibration complete: ${result.frame_count} frames, baseline ready.`);
    } catch (error) {
      this._setCalibrationStatus(error.message, true);
    }
  }

  async _resetCalibration() {
    try {
      const status = await apiService.get('/api/v1/calibration/status');
      const result = await apiService.post('/api/v1/calibration/reset', {
        boot_epoch: status.boot_epoch,
      });
      if (!result?.success) throw new Error(result?.error || 'Calibration reset failed.');
      this._stopCalibrationPolling();
      this._calibrationIdentity = null;
      this._setCalibrationButtons(false);
      this._setCalibrationProgress({ source_node_ids: [] });
      this._setCalibrationStatus('Calibration reset. Start a new empty-room capture after live frames arrive.');
    } catch (error) {
      this._setCalibrationStatus(error.message, true);
    }
  }

  _startCalibrationPolling() {
    this._stopCalibrationPolling();
    this._calibrationTimer = setInterval(() => void this._refreshCalibrationStatus(), 1000);
  }

  _stopCalibrationPolling() {
    if (this._calibrationTimer) clearInterval(this._calibrationTimer);
    this._calibrationTimer = null;
  }

  async _refreshCalibrationStatus() {
    try {
      const status = await apiService.get('/api/v1/calibration/status');
      if (status?.session_id && status?.binding_digest && status?.source_node_ids?.length) {
        this._calibrationIdentity = {
          boot_epoch: status.boot_epoch,
          session_id: status.session_id,
          binding_digest: status.binding_digest,
          source_node_ids: status.source_node_ids,
        };
      }
      const active = Boolean(status?.session_id);
      this._setCalibrationButtons(active);
      this._setCalibrationProgress(status);
      const missing = status?.missing_source_node_ids?.length
        ? `; missing nodes: ${status.missing_source_node_ids.join(',')}`
        : '';
      this._setCalibrationStatus(
        `${status?.status || 'none'} — ${status?.frame_count || 0} frames, ` +
        `${Number(status?.elapsed_s || 0).toFixed(0)}s${missing}`
      );
    } catch {
      this._setCalibrationStatus('Calibration status unavailable; verify that RuView is running.', true);
    }
  }

  // ---- HUD update --------------------------------------------------------

  _updateHUD(data) {
    const f = data.features || {};
    const c = data.classification || {};

    // Node count
    const nodeCount = (data.nodes || []).length;
    const countEl = this.container.querySelector('#sensingNodeCount');
    if (countEl) countEl.textContent = String(nodeCount);

    // RSSI
    this._setText('sensingRssi', `${(f.mean_rssi || -80).toFixed(1)} dBm`);
    this._setText('sensingSource', data.source || '');

    // Bars (scale to 0-100%)
    this._setBar('barVariance', f.variance, 10, 'valVariance', f.variance);
    this._setBar('barMotion', f.motion_band_power, 0.5, 'valMotion', f.motion_band_power);
    this._setBar('barBreath', f.breathing_band_power, 0.3, 'valBreath', f.breathing_band_power);
    this._setBar('barSpectral', f.spectral_power, 2.0, 'valSpectral', f.spectral_power);

    // Classification
    const label = this.container.querySelector('#classLabel');
    if (label) {
      const level = (c.motion_level || 'absent').toUpperCase();
      label.textContent = level;
      label.className = 'sensing-class-label ' + (c.motion_level || 'absent');
    }

    const confPct = ((c.confidence || 0) * 100).toFixed(0);
    this._setBar('barConfidence', c.confidence, 1.0, 'valConfidence', confPct + '%');

    // Details
    this._setText('valDomFreq', (f.dominant_freq_hz || 0).toFixed(3) + ' Hz');
    this._setText('valChangePoints', String(f.change_points || 0));
    const srcLabel = (data.source === 'simulated' || data.source === 'simulate') ? 'sim' : data.source || 'live';
    this._setText('valSampleRate', srcLabel);

    // Sparkline
    this._drawSparkline();
  }

  _setText(id, text) {
    const el = this.container.querySelector('#' + id);
    if (el) el.textContent = text;
  }

  _setBar(barId, value, maxVal, valId, displayVal) {
    const bar = this.container.querySelector('#' + barId);
    if (bar) {
      const pct = Math.min(100, Math.max(0, ((value || 0) / maxVal) * 100));
      bar.style.width = pct + '%';
    }
    if (valId && displayVal != null) {
      const el = this.container.querySelector('#' + valId);
      if (el) el.textContent = typeof displayVal === 'number' ? displayVal.toFixed(3) : displayVal;
    }
  }

  _drawSparkline() {
    const canvas = this.container.querySelector('#sensingSparkline');
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const history = sensingService.getRssiHistory();
    if (history.length < 2) return;

    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    const min = Math.min(...history) - 2;
    const max = Math.max(...history) + 2;
    const range = max - min || 1;

    ctx.beginPath();
    ctx.strokeStyle = '#32b8c6';
    ctx.lineWidth = 1.5;

    for (let i = 0; i < history.length; i++) {
      const x = (i / (history.length - 1)) * w;
      const y = h - ((history[i] - min) / range) * h;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }

  // ---- Per-node panels ---------------------------------------------------

  _updateNodePanels(data) {
    const container = this.container.querySelector('#nodeStatusContainer');
    if (!container) return;
    const nodeFeatures = data.node_features || [];
    if (nodeFeatures.length === 0) {
      container.textContent = '';
      const msg = document.createElement('div');
      msg.style.cssText = 'color:#888;font-size:12px;padding:8px;';
      msg.textContent = 'No nodes detected';
      container.appendChild(msg);
      return;
    }
    const NODE_COLORS = ['#00ccff', '#ff6600', '#00ff88', '#ff00cc', '#ffcc00', '#8800ff', '#00ffcc', '#ff0044'];
    container.textContent = '';
    for (const nf of nodeFeatures) {
      const color = NODE_COLORS[nf.node_id % NODE_COLORS.length];
      const statusColor = nf.stale ? '#888' : '#0f0';

      const row = document.createElement('div');
      row.style.cssText = `display:flex;align-items:center;gap:8px;padding:6px 8px;margin-bottom:4px;background:rgba(255,255,255,0.03);border-radius:6px;border-left:3px solid ${color};`;

      const idCol = document.createElement('div');
      idCol.style.minWidth = '50px';
      const nameEl = document.createElement('div');
      nameEl.style.cssText = `font-size:11px;font-weight:600;color:${color};`;
      nameEl.textContent = 'Node ' + nf.node_id;
      const statusEl = document.createElement('div');
      statusEl.style.cssText = `font-size:9px;color:${statusColor};`;
      statusEl.textContent = nf.stale ? 'STALE' : 'ACTIVE';
      idCol.appendChild(nameEl);
      idCol.appendChild(statusEl);

      const metricsCol = document.createElement('div');
      metricsCol.style.cssText = 'flex:1;font-size:10px;color:#aaa;';
      metricsCol.textContent = (nf.rssi_dbm || -80).toFixed(0) + ' dBm · var ' + (nf.features?.variance || 0).toFixed(1);

      const classCol = document.createElement('div');
      classCol.style.cssText = 'font-size:10px;font-weight:600;color:#ccc;';
      const motion = (nf.classification?.motion_level || 'absent').toUpperCase();
      const conf = ((nf.classification?.confidence || 0) * 100).toFixed(0);
      classCol.textContent = motion + ' ' + conf + '%';

      row.appendChild(idCol);
      row.appendChild(metricsCol);
      row.appendChild(classCol);
      container.appendChild(row);
    }
  }

  // ---- Resize ------------------------------------------------------------

  _setupResize() {
    const viewport = this.container.querySelector('#sensingViewport');
    if (!viewport || !window.ResizeObserver) return;

    this._resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (this.splatRenderer) {
          this.splatRenderer.resize(entry.contentRect.width, entry.contentRect.height);
        }
      }
    });
    this._resizeObserver.observe(viewport);
  }

  // ---- Cleanup -----------------------------------------------------------

  dispose() {
    if (this._unsubData) this._unsubData();
    if (this._unsubState) this._unsubState();
    if (this._resizeObserver) this._resizeObserver.disconnect();
    if (this.splatRenderer) this.splatRenderer.dispose();
    sensingService.stop();
  }
}
