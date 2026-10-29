"""Offline regression for the live experiment driver's completion gate."""
import unittest

from pty_ux4_live import command_completed


class CompletionTests(unittest.TestCase):
    def test_inspector_help_does_not_complete_pending_command(self):
        screen = ('Inspector: F4 commands; F6 focus\n'
                  'F4 command is available after the request\n'
                  '> :scope document-ask explain')
        self.assertFalse(command_completed(screen))

    def test_restored_footer_completes_command(self):
        self.assertTrue(command_completed('output\nF1 actions • F4 command • Ctrl-Q exit'))

    def test_empty_screen_is_not_completion(self):
        self.assertFalse(command_completed(''))


if __name__ == '__main__':
    unittest.main()
