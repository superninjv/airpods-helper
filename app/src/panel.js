"use strict";

var AirPodsPanel = {
  deviceName(address, devices) {
    if (!address) return "AirPods";
    return devices.find(device => device.address.toUpperCase() === address.toUpperCase())?.name || "AirPods";
  },
  wireResize(handles, startResize) {
    for (const handle of handles) handle.addEventListener("mousedown", event => {
      if (event.button !== 0) return;
      event.preventDefault();
      startResize(handle.dataset.resize);
    });
  },
  wireTabs(tabs, pages, onSelect) {
    function select(name) {
      for (const tab of tabs) {
        const active = tab.dataset.page === name;
        tab.setAttribute("aria-selected", String(active));
        tab.tabIndex = active ? 0 : -1;
      }
      for (const page of pages) page.hidden = page.dataset.page !== name;
      onSelect(name);
    }
    tabs.forEach((tab, index) => {
      tab.addEventListener("click", () => select(tab.dataset.page));
      tab.addEventListener("keydown", event => {
        let next;
        if (event.key === "ArrowRight") next = (index + 1) % tabs.length;
        else if (event.key === "ArrowLeft") next = (index + tabs.length - 1) % tabs.length;
        else if (event.key === "Home") next = 0;
        else if (event.key === "End") next = tabs.length - 1;
        else return;
        event.preventDefault();
        select(tabs[next].dataset.page);
        tabs[next].focus();
      });
    });
    select("controls");
    return select;
  }
};

if (typeof module !== "undefined") module.exports = AirPodsPanel;
