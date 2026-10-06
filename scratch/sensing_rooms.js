  async _loadRooms() {
    const sel = this.container.querySelector('#sensingRoomSelect');
    if (!sel) return;
    try {
      const res = await fetch('/api/v1/rooms');
      if (res.ok) {
        const data = await res.json();
        sel.innerHTML = '<option value="">No Room Selected</option>' + data.rooms.map(r => 
          `<option value="${r.id}">${r.name}</option>`
        ).join('');
        if (data.active_room_id) {
          sel.value = data.active_room_id;
          const activeRoom = data.rooms.find(r => r.id === data.active_room_id);
          if (activeRoom) {
            const input = this.container.querySelector('#calibrationNodeIds');
            if (input) input.value = activeRoom.node_ids.join(',');
          }
        }
      }
      sel.addEventListener('change', async (e) => {
        const id = e.target.value;
        const resp = await fetch('/api/v1/rooms/active', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id: id || null })
        });
        if (resp.ok && id) {
          const res2 = await fetch('/api/v1/rooms');
          const data2 = await res2.json();
          const r = data2.rooms.find(r => r.id === id);
          if (r) {
            const input = this.container.querySelector('#calibrationNodeIds');
            if (input) input.value = r.node_ids.join(',');
          }
        }
      });
    } catch (e) {
      console.error('Failed to load rooms for SensingTab', e);
    }
  }
