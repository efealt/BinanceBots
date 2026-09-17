window.ChartIndicatorToolbar = class ChartIndicatorToolbar {
  constructor(container, layer) {
    this.container = container;
    this.layer = layer;
    this.typeSelect = container.querySelector("[data-indicator-type]");
    this.addButton = container.querySelector("[data-indicator-add]");
    this.list = container.querySelector("[data-indicator-list]");

    this.addButton.addEventListener("click", () => {
      this.layer.add(this.typeSelect.value);
      this.render();
    });

    this.render();
  }

  render() {
    const active = new Map(this.layer.snapshots().map((item) => [item.type, item]));
    for (const option of this.typeSelect.options) option.disabled = active.has(option.value);
    this.addButton.disabled = active.size === this.typeSelect.options.length;
    const available = [...this.typeSelect.options].find((option) => !option.disabled);
    if (available) this.typeSelect.value = available.value;

    this.list.replaceChildren();
    if (!active.size) {
      const empty = document.createElement("span");
      empty.className = "muted-copy";
      empty.textContent = "No indicators added";
      this.list.appendChild(empty);
      return;
    }

    for (const [type, item] of active) this.list.appendChild(this.createItem(type, item));
  }

  createItem(type, item) {
    const definition = window.ChartIndicatorDefinitions.get(type);
    const card = document.createElement("div");
    card.className = "indicator-item";

    const heading = document.createElement("div");
    heading.className = "indicator-item-heading";

    const title = document.createElement("strong");
    title.textContent = definition.label;
    heading.appendChild(title);

    const remove = document.createElement("button");
    remove.className = "button button--secondary indicator-remove";
    remove.type = "button";
    remove.textContent = "Remove";
    remove.addEventListener("click", () => {
      this.layer.remove(type);
      this.render();
    });
    heading.appendChild(remove);
    card.appendChild(heading);

    const parameters = document.createElement("div");
    parameters.className = "indicator-item-parameters";
    for (const parameter of definition.parameters) {
      const field = document.createElement("label");
      field.className = "field";
      field.textContent = parameter.label;

      const input = document.createElement("input");
      input.className = "field-control";
      input.type = "number";
      input.min = parameter.min;
      input.max = parameter.max;
      input.step = parameter.step;
      input.value = item.parameters[parameter.key];
      input.addEventListener("change", () => {
        const snapshot = this.layer.updateParameters(type, {
          [parameter.key]: input.value,
        });
        input.value = snapshot.parameters[parameter.key];
      });

      field.appendChild(input);
      parameters.appendChild(field);
    }
    card.appendChild(parameters);
    return card;
  }
};
