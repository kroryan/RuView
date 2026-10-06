// Hardware Tab Component
//
// This tab is deliberately hardware-only. It reads the live CSI endpoint and
// never fabricates antenna values when the ESP32 stream is unavailable.

import { apiService } from '../services/api.service.js';

export class HardwareTab {
  constructor(containerElement) {
    this.container = containerElement;
    this.refreshTimer = null;
  }

  init() {
    this._refresh();
    this.refreshTimer = setInterval(() => this._refresh(), 2000);
  }

  async _refresh() {
    try {
      const [status, latest] = await Promise.all([
        apiService.get('/api/v1/status'),
        apiService.get('/api/v1/sensing/latest'),
      ]);
      this._render(status, latest);
    } catch {
      this._renderUnavailable('Hardware status unavailable — no measured CSI data.');
    }
  }

  _render(status, latest) {
    const sourceState = status?.source_state || 'disconnected';
    const nodes = Array.isArray(latest?.nodes) ? latest.nodes : [];
    const live = (sourceState === 'live_verified' || sourceState === 'live_unverified') && nodes.length > 0;
    const banner = this.container.querySelector('#hardwareSourceBanner');
    const state = this.container.querySelector('#hardwareSourceState');
    const nodeList = this.container.querySelector('#hardwareNodeList');

    if (banner) {
      banner.textContent = live ? 'LIVE — MEASURED ESP32 CSI' : 'NO LIVE HARDWARE DATA';
      banner.className = `hardware-source-banner ${live ? 'hardware-source-live' : 'hardware-source-waiting'}`;
    }
    if (state) {
      state.textContent = live ? `${nodes.length} ESP32 node(s) transmitting` : 'Waiting for measured CSI frames';
    }
    if (nodeList) {
      nodeList.replaceChildren();
      if (!nodes.length) {
        const empty = document.createElement('p');
        empty.textContent = 'No ESP32 node is currently providing CSI data.';
        nodeList.appendChild(empty);
      } else {
        nodes.forEach((node) => nodeList.appendChild(this._nodeRow(node)));
      }
    }

    const node = nodes[0] || {};
    const amplitude = Array.isArray(node.amplitude) && node.amplitude.length
      ? node.amplitude.reduce((sum, value) => sum + Number(value || 0), 0) / node.amplitude.length
      : null;
    this._setText('hardwareNodeCount', String(nodes.length));
    this._setText('hardwareSubcarriers', node.subcarrier_count ? String(node.subcarrier_count) : '—');
    this._setText('hardwareRate', node.sync?.csi_fps_ema ? `${Number(node.sync.csi_fps_ema).toFixed(1)} Hz` : '—');
    this._setText('hardwareAmplitude', amplitude === null ? '—' : amplitude.toFixed(2));
    this._setText('hardwarePhase', 'Unavailable — amplitude-only CSI');
  }

  _nodeRow(node) {
    const row = document.createElement('div');
    row.className = 'hardware-node-row';
    const id = document.createElement('strong');
    id.textContent = `Node ${node.node_id ?? '—'}`;
    const detail = document.createElement('span');
    const rssi = Number.isFinite(Number(node.rssi_dbm)) ? `${Number(node.rssi_dbm).toFixed(0)} dBm` : 'RSSI —';
    detail.textContent = `${node.stale ? 'stale' : 'live'} · ${rssi} · ${node.subcarrier_count || '—'} subcarriers`;
    row.append(id, detail);
    return row;
  }

  _renderUnavailable(message) {
    const banner = this.container.querySelector('#hardwareSourceBanner');
    const state = this.container.querySelector('#hardwareSourceState');
    const nodeList = this.container.querySelector('#hardwareNodeList');
    if (banner) {
      banner.textContent = 'NO LIVE HARDWARE DATA';
      banner.className = 'hardware-source-banner hardware-source-waiting';
    }
    if (state) state.textContent = message;
    if (nodeList) {
      nodeList.replaceChildren();
      const empty = document.createElement('p');
      empty.textContent = message;
      nodeList.appendChild(empty);
    }
    ['hardwareNodeCount', 'hardwareSubcarriers', 'hardwareRate', 'hardwareAmplitude', 'hardwarePhase']
      .forEach((id) => this._setText(id, '—'));
  }

  _setText(id, value) {
    const element = this.container.querySelector(`#${id}`);
    if (element) element.textContent = value;
  }

  dispose() {
    if (this.refreshTimer) clearInterval(this.refreshTimer);
    this.refreshTimer = null;
  }
}
