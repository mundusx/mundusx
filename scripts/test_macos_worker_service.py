import importlib.util
from pathlib import Path
import tempfile
import unittest
spec=importlib.util.spec_from_file_location("service",Path(__file__).with_name("macos-worker-service.py"))
service=importlib.util.module_from_spec(spec);spec.loader.exec_module(service)
class ServiceTests(unittest.TestCase):
 def test_stop_or_missing_intent_never_starts_worker(self):
  for config in [{},{"connected":False},{"connected":"true"},{"paused":False}]:self.assertFalse(service.should_run(config))
 def test_pause_keeps_connected_intent_without_changing_it(self):
  c={"connected":True,"paused":True};self.assertTrue(service.should_run(c));self.assertTrue(c["paused"])
 def test_only_exact_worker_command_is_adopted(self):
  exe=Path("/Users/test/Library/Application Support/OpenGPU/bin/opengpu-node-agent")
  output=f"12 python unrelated.py\n34 {exe} run\n56 {exe} worker\n"
  self.assertEqual(service.existing_worker(output,exe),34)
  self.assertIsNone(service.existing_worker(output,Path("/different/worker")))
 def test_invalid_config_fails_closed_and_pid_is_written(self):
  with tempfile.TemporaryDirectory() as d:
   home=Path(d);(home/"config.json").write_text("invalid")
   self.assertEqual(service.read_intent(home),{})
   service.record_pid(home,42);self.assertEqual((home/"node-agent.pid").read_text(),"42")
if __name__=="__main__":unittest.main()
