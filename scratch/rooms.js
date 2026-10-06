  async initRooms() {
    const sel = document.getElementById('room-select');
    if (!sel) return;
    
    const res = await fetch('/api/v1/rooms');
    if (res.ok) {
      const data = await res.json();
      sel.innerHTML = '<option value="">No Room Selected</option>' + data.rooms.map(r => 
        `<option value="${r.id}">${r.name}</option>`
      ).join('');
      if (data.active_room_id) {
        sel.value = data.active_room_id;
      }
      document.getElementById('room-area').style.display = 'block';
    }

    sel.addEventListener('change', async (e) => {
      const id = e.target.value;
      await fetch('/api/v1/rooms/active', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id: id || null })
      });
    });
  }
