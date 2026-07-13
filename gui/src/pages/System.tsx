import { useState } from "react";
import ExpertAgents from "../components/ExpertAgents";
import ExpertResources from "../components/ExpertResources";
import ExpertSecret from "../components/ExpertSecret";
import ExpertStore from "../components/ExpertStore";
import ExpertSystem from "../components/ExpertSystem";
import ExpertTrigger from "../components/ExpertTrigger";

type Section = "agents" | "resources" | "triggers" | "stores" | "secrets" | "runtime";

const sections: Array<[Section, string]> = [
  ["agents", "Agents"], ["resources", "Workflows & Resources"], ["triggers", "Triggers"],
  ["stores", "Stores"], ["secrets", "Secrets"], ["runtime", "Runtime & Connection"],
];

export default function System({ initialSection }: { initialSection?: string }) {
  const valid = sections.some(([key]) => key === initialSection);
  const [section, setSection] = useState<Section>(valid ? initialSection as Section : "agents");
  return <main aria-labelledby="system-title">
    <header className="page-heading">
      <div><h1 id="system-title" className="page-title">System</h1><p>Agents, resources, policies, stores and runtime controls.</p></div>
    </header>
    <div className="system-layout">
      <nav className="section-nav" aria-label="System sections">
        {sections.map(([key, label]) => <button key={key} className={`btn ${section === key ? "btn-primary" : "btn-ghost"}`} onClick={() => setSection(key)}>{label}</button>)}
      </nav>
      <section className="liquid-glass system-panel" aria-live="polite">
        {section === "agents" && <ExpertAgents />}
        {section === "resources" && <ExpertResources />}
        {section === "triggers" && <ExpertTrigger />}
        {section === "stores" && <ExpertStore />}
        {section === "secrets" && <ExpertSecret />}
        {section === "runtime" && <ExpertSystem />}
      </section>
    </div>
  </main>;
}
