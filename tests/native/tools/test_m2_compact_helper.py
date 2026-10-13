"""Portable numerical verdict controls; never execute a helper."""
import copy
import unittest
from m2_compact_helper import numerical_verdict


class ComparisonControls(unittest.TestCase):
    def setUp(self):
        self.plan={'comparison':{'variants':['qualification','qualification-startup'],
                  'wallLimitMs':250,'discriminatingFirstImprovementMs':11}}
        self.rows=[{'variant':variant,'arm':arm,'copy':pair,'call':call,'semanticPass':True,
                    'launchToReapMs':240 if arm=='release' else 100,
                    'phaseMs':{'launchToMain':220 if arm=='release' else 80}}
                   for variant in self.plan['comparison']['variants']
                   for arm,pair in [('release',0),('capture-compact',0),('capture-compact',1),('release',1)] for call in range(2)]

    def test_complete_discriminating_is_still_not_full_go(self):
        self.assertEqual(numerical_verdict(self.rows,self.plan),'NUMERICAL_GO_SEMANTIC_CONTROLS_PENDING')

    def test_every_compact_wall_or_semantic_failure_stops(self):
        for index,row in enumerate(self.rows):
            mutated=copy.deepcopy(self.rows);mutated[index]['semanticPass']=False
            self.assertEqual(numerical_verdict(mutated,self.plan),'NO_GO')
            if row['arm']=='capture-compact':
                mutated=copy.deepcopy(self.rows);mutated[index]['launchToReapMs']=250
                self.assertEqual(numerical_verdict(mutated,self.plan),'NO_GO')

    def test_no_favorable_pair_selection_or_warm_only_benefit(self):
        for index,row in enumerate(self.rows):
            if row['arm']=='capture-compact' and row['call']==0:
                mutated=copy.deepcopy(self.rows);mutated[index]['launchToReapMs']=239
                self.assertEqual(numerical_verdict(mutated,self.plan),'INCONCLUSIVE_BENEFIT')

    def test_missing_rows_cannot_go(self):
        for end in range(16):
            self.assertEqual(numerical_verdict(self.rows[:end],self.plan),'INCOMPLETE')

    def test_phase_must_support_startup_improvement(self):
        self.rows[10]['phaseMs']['launchToMain']=240
        self.assertEqual(numerical_verdict(self.rows,self.plan),'INCONCLUSIVE_BENEFIT')


if __name__=='__main__': unittest.main(verbosity=2)
