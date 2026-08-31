import { createRouter, createWebHistory } from 'vue-router';
import OverviewView from '../views/OverviewView.vue';
import ClusterView from '../views/ClusterView.vue';
import ServicesView from '../views/ServicesView.vue';
import StoreView from '../views/StoreView.vue';
import ConfigView from '../views/ConfigView.vue';
import EnvView from '../views/EnvView.vue';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'overview',
      component: OverviewView,
    },
    {
      path: '/cluster',
      name: 'cluster',
      component: ClusterView,
    },
    {
      path: '/config',
      name: 'config',
      component: ConfigView,
    },
    {
      path: '/services',
      name: 'services',
      component: ServicesView,
    },
    {
      path: '/store',
      name: 'store',
      component: StoreView,
    },
    {
      path: '/logs',
      redirect: '/config',
    },
    {
      path: '/events',
      redirect: '/',
    },
    {
      path: '/env',
      name: 'env',
      component: EnvView,
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
});

export default router;
